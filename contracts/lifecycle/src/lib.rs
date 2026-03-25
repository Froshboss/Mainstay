#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, String, Symbol, Vec};

#[contracttype]
#[derive(Clone)]
pub struct MaintenanceRecord {
    pub asset_id: u64,
    pub task_type: Symbol,
    pub notes: String,
    pub engineer: Address,
    pub timestamp: u64,
}

fn history_key(asset_id: u64) -> (Symbol, u64) {
    (symbol_short!("HIST"), asset_id)
}

fn score_key(asset_id: u64) -> (Symbol, u64) {
    (symbol_short!("SCORE"), asset_id)
}

fn registry_key() -> Symbol {
    symbol_short!("REGISTRY")
}

#[contract]
pub struct Lifecycle;

#[contractimpl]
impl Lifecycle {
    /// Must be called once after deployment to set the asset-registry contract address.
    pub fn initialize(env: Env, asset_registry: Address) {
        env.storage().instance().set(&registry_key(), &asset_registry);
    }

    pub fn submit_maintenance(
        env: Env,
        asset_id: u64,
        task_type: Symbol,
        notes: String,
        engineer: Address,
    ) {
        engineer.require_auth();

        // Validate asset exists in the registry (panics with "asset not found" if not)
        let registry: Address = env
            .storage()
            .instance()
            .get(&registry_key())
            .expect("registry not set");
        let registry_client = asset_registry::AssetRegistryClient::new(&env, &registry);
        registry_client.get_asset(&asset_id);

        let record = MaintenanceRecord {
            asset_id,
            task_type,
            notes,
            engineer,
            timestamp: env.ledger().timestamp(),
        };

        let mut history: Vec<MaintenanceRecord> = env
            .storage()
            .persistent()
            .get(&history_key(asset_id))
            .unwrap_or(Vec::new(&env));
        history.push_back(record);
        env.storage().persistent().set(&history_key(asset_id), &history);

        let score: u32 = env
            .storage()
            .persistent()
            .get(&score_key(asset_id))
            .unwrap_or(0u32);
        let new_score = (score + 5).min(100);
        env.storage().persistent().set(&score_key(asset_id), &new_score);
    }

    pub fn get_maintenance_history(env: Env, asset_id: u64) -> Vec<MaintenanceRecord> {
        env.storage()
            .persistent()
            .get(&history_key(asset_id))
            .unwrap_or(Vec::new(&env))
    }

    pub fn get_last_service(env: Env, asset_id: u64) -> MaintenanceRecord {
        let history: Vec<MaintenanceRecord> = env
            .storage()
            .persistent()
            .get(&history_key(asset_id))
            .expect("no maintenance history");
        history.last().expect("no records")
    }

    pub fn get_collateral_score(env: Env, asset_id: u64) -> u32 {
        env.storage()
            .persistent()
            .get(&score_key(asset_id))
            .unwrap_or(0)
    }

    pub fn is_collateral_eligible(env: Env, asset_id: u64) -> bool {
        Self::get_collateral_score(env, asset_id) >= 50
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use asset_registry::{AssetRegistry, AssetRegistryClient};
    use soroban_sdk::{symbol_short, testutils::Address as _, Env, String};

    fn setup(env: &Env) -> (LifecycleClient<'_>, AssetRegistryClient<'_>) {
        let registry_id = env.register(AssetRegistry, ());
        let registry_client = AssetRegistryClient::new(env, &registry_id);

        let lifecycle_id = env.register(Lifecycle, ());
        let client = LifecycleClient::new(env, &lifecycle_id);
        client.initialize(&registry_id);

        (client, registry_client)
    }

    #[test]
    fn test_submit_and_score() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, registry_client) = setup(&env);

        let owner = Address::generate(&env);
        let asset_id = registry_client.register_asset(
            &symbol_short!("GENSET"),
            &String::from_str(&env, "Caterpillar 3516"),
            &owner,
        );

        let engineer = Address::generate(&env);
        for _ in 0..10 {
            client.submit_maintenance(
                &asset_id,
                &symbol_short!("OIL_CHG"),
                &String::from_str(&env, "Routine oil change"),
                &engineer,
            );
        }

        assert_eq!(client.get_collateral_score(&asset_id), 50);
        assert!(client.is_collateral_eligible(&asset_id));
        assert_eq!(client.get_maintenance_history(&asset_id).len(), 10);
    }

    #[test]
    #[should_panic(expected = "asset not found")]
    fn test_submit_maintenance_nonexistent_asset() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _) = setup(&env);

        let engineer = Address::generate(&env);
        // asset_id 999 was never registered — must panic
        client.submit_maintenance(
            &999u64,
            &symbol_short!("OIL_CHG"),
            &String::from_str(&env, "Should fail"),
            &engineer,
        );
    }
}
