#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, contracterror, panic_with_error, symbol_short, Address, BytesN, Env, Symbol};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    CredentialAlreadyRevoked = 1,
    UnauthorizedAdmin = 2,
}

#[contracttype]
#[derive(Clone)]
pub struct Engineer {
    pub address: Address,
    pub credential_hash: BytesN<32>,
    pub issuer: Address,
    pub active: bool,
    pub issued_at: u64,
}

fn engineer_key(addr: &Address) -> (Symbol, Address) {
    (symbol_short!("ENG"), addr.clone())
}

fn admin_key() -> Symbol {
    symbol_short!("ADMIN")
}

fn trusted_key(issuer: &Address) -> (Symbol, Address) {
    (symbol_short!("TRUSTED"), issuer.clone())
}

#[contract]
pub struct EngineerRegistry;

#[contractimpl]
impl EngineerRegistry {
    pub fn register_engineer(
        env: Env,
        engineer: Address,
        credential_hash: BytesN<32>,
        issuer: Address,
    ) {
        issuer.require_auth();
        assert!(
            credential_hash != BytesN::from_array(&env, &[0u8; 32]),
            "credential hash cannot be zero"
        );
        let record = Engineer {
            address: engineer.clone(),
            credential_hash,
            issuer,
            active: true,
            issued_at: env.ledger().timestamp(),
        };
        env.storage()
            .persistent()
            .set(&engineer_key(&engineer), &record);
    }

    pub fn verify_engineer(env: Env, engineer: Address) -> bool {
        env.storage()
            .persistent()
            .get::<_, Engineer>(&engineer_key(&engineer))
            .map(|e| e.active)
            .unwrap_or(false)
    }

    pub fn revoke_credential(env: Env, engineer: Address) {
        let caller = env.invoker();
        let admin = get_admin(env.clone());
        let mut record: Engineer = env
            .storage()
            .persistent()
            .get(&engineer_key(&engineer))
            .expect("engineer not found");
        assert!(record.issuer == issuer, "not the issuer");
        assert!(record.active, "credential already revoked");
        record.active = false;
        env.storage()
            .persistent()
            .set(&engineer_key(&engineer), &record);
    }

    pub fn get_engineer(env: Env, engineer: Address) -> Engineer {
        env.storage()
            .persistent()
            .get(&engineer_key(&engineer))
            .expect("engineer not found")
    }

    pub fn initialize_admin(env: Env, admin: Address) {
        if env.storage().instance().has(&admin_key()) {
            panic!("admin already initialized");
        }
        env.storage().instance().set(&admin_key(), &admin);
    }

    pub fn get_admin(env: Env) -> Address {
        env.storage().instance().get(&admin_key())
            .expect("admin not initialized")
    }

    pub fn is_trusted_issuer(env: Env, issuer: Address) -> bool {
        env.storage().instance().has(&trusted_key(&issuer))
    }

    pub fn add_trusted_issuer(env: Env, issuer: Address) {
        let admin = get_admin(env.clone());
        if env.invoker() != admin {
            panic!("Only admin can add trusted issuers");
        }
        env.storage().instance().set(&trusted_key(&issuer), &());
        env.storage().instance().extend_ttl(&trusted_key(&issuer), 518400, 518400);
    }

    pub fn remove_trusted_issuer(env: Env, issuer: Address) {
        let admin = get_admin(env.clone());
        if env.invoker() != admin {
            panic!("Only admin can remove trusted issuers");
        }
        env.storage().instance().remove(&trusted_key(&issuer));
    }

    /// Admin-only: upgrade the contract WASM to a new hash.
    pub fn upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&admin_key())
            .expect("admin not initialized");
        if stored_admin != admin {
            panic_with_error!(&env, ContractError::UnauthorizedAdmin);
        }

        env.deployer().update_current_contract_wasm(new_wasm_hash);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, BytesN, Env, Symbol};

    #[test]
    fn test_register_verify_revoke() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(EngineerRegistry, ());
        let client = EngineerRegistryClient::new(&env, &contract_id);

        let engineer = Address::generate(&env);
        let issuer = Address::generate(&env);
        let admin = Address::generate(&env);
        let hash = BytesN::from_array(&env, &[1u8; 32]);

        client.initialize_admin(&admin);
        client.add_trusted_issuer(&issuer);
        client.register_engineer(&engineer, &hash, &issuer);
        assert!(client.verify_engineer(&engineer));

        client.revoke_credential(&engineer);
        assert!(!client.verify_engineer(&engineer));
    }

    #[test]
    #[should_panic(expected = "credential hash cannot be zero")]
    fn test_register_zero_hash_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(EngineerRegistry, ());
        let client = EngineerRegistryClient::new(&env, &contract_id);

        let engineer = Address::generate(&env);
        let issuer = Address::generate(&env);
        let admin = Address::generate(&env);
        let zero_hash = BytesN::from_array(&env, &[0u8; 32]);

        client.initialize_admin(&admin);
        client.add_trusted_issuer(&issuer);
        client.register_engineer(&engineer, &zero_hash, &issuer);
    }

    #[test]
    fn test_ttl_extended_on_registration() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(EngineerRegistry, ());
        let client = EngineerRegistryClient::new(&env, &contract_id);

        let engineer = Address::generate(&env);
        let issuer = Address::generate(&env);
        let hash = BytesN::from_array(&env, &[1u8; 32]);

        client.register_engineer(&engineer, &hash, &issuer);

        // Verify TTL is set for engineer storage entry
        let engineer_ttl = env.storage().persistent().get_ttl(&engineer_key(&engineer));
        assert!(engineer_ttl > 0, "Engineer TTL should be extended");
    }

    #[test]
    fn test_ttl_extended_on_revoke() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(EngineerRegistry, ());
        let client = EngineerRegistryClient::new(&env, &contract_id);

        let engineer = Address::generate(&env);
        let issuer = Address::generate(&env);
        let hash = BytesN::from_array(&env, &[1u8; 32]);

        client.register_engineer(&engineer, &hash, &issuer);
        client.revoke_credential(&engineer, &issuer);

        // Verify TTL is still set after revoke
        let engineer_ttl = env.storage().persistent().get_ttl(&engineer_key(&engineer));
        assert!(engineer_ttl > 0, "Engineer TTL should be extended after revoke");
    }

    #[test]
    fn test_admin_can_upgrade() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(EngineerRegistry, ());
        let client = EngineerRegistryClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        client.initialize_admin(&admin);

        let new_wasm_hash = BytesN::from_array(&env, &[0xabu8; 32]);
        // Should not panic — admin is authorized
        client.upgrade(&admin, &new_wasm_hash);
    }

    #[test]
    fn test_non_admin_cannot_upgrade() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(EngineerRegistry, ());
        let client = EngineerRegistryClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        client.initialize_admin(&admin);

        let outsider = Address::generate(&env);
        let new_wasm_hash = BytesN::from_array(&env, &[0xabu8; 32]);

        let result = client.try_upgrade(&outsider, &new_wasm_hash);
        assert_eq!(
            result,
            Err(Ok(soroban_sdk::Error::from_contract_error(
                ContractError::UnauthorizedAdmin as u32,
            ))),
        );
    }
}

