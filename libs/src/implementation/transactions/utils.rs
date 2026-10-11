use ethers::signers::LocalWallet;

use book::{ err_utils::{ ErrStr, err_or };
use libs::types::blockchains::Blockchain;

pub async fn load_signer(blockchain: &Blockchain, expected_address: &str,
                         keystore_path: &str) -> ErrStr<LocalWallet> {
    let password = match std::env::var("KEYSTORE_PASSWORD") {
        Ok(pw) => pw,
        Err(_) => err_or(rpassword::prompt_password("Keystore password: "),
                         "Could not read password. No funds moved.")?
    };  
    let wallet =
       err_or(LocalWallet::decrypt_keystore(&keystore_path, &password),
              &format!("Could not decrypt keystore, path {keystore_path}.
No funds moved."))?
             .with_chain_id(blockchain.chain_id());
    let derived = format!("{:?}", wallet.address());
    if !derived.eq_ignore_ascii_case(expected_address) {
        Err(format!("Keystore address ({derived}) does not match expected
address ({expected_address}) — refusing to proceed. No funds moved."
        ))
    } else { 
       Ok(wallet)        
    }
}  

