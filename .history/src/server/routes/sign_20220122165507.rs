use actix_web::{HttpResponse, web::{Data, Json}};
use bundlr_sdk::{deep_hash::{deep_hash, DeepHashChunk, ONE_AS_BUFFER}, JWK, deep_hash_sync::deep_hash_sync};
use bytes::Bytes;
use data_encoding::{BASE64URL, BASE64URL_NOPAD};
use diesel::RunQueryDsl;
use jsonwebkey::JsonWebKey;
use lazy_static::lazy_static;
use openssl::{sign, hash::MessageDigest, rsa::Padding, pkey::{PKey, Private, Public}};
use redis::AsyncCommands;
use reool::{RedisPool, PoolDefault};
use serde::{Serialize, Deserialize};

use crate::{server::error::ValidatorServerError, types::DbPool, database::{schema::transactions::dsl::*, models::{Transaction, NewTransaction}}, consts::{BUNDLR_AS_BUFFER, VALIDATOR_AS_BUFFER}};

#[derive(Deserialize)]
pub struct UnsignedBody {
    id: String,
    signature: String,
    block: u128
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SignedBody {
    id: String,
    signature: String,
    block: u128,
    validator_address: String,
    validator_signature: String
}

lazy_static! {
    static ref BUNDLER_PUBLIC: Vec<u8> = {
        let var = std::env::var("BUNDLER_PUBLIC").unwrap();
        var.as_bytes().to_vec()
    };
    static ref BUNDLER_ADDRESS: String = BASE64URL.encode(std::env::var("BUNDLER_PUBLIC").unwrap().as_bytes());
}

pub async fn sign_route(db: Data<DbPool>, redis: Data<RedisPool>, body: Json<UnsignedBody>) -> actix_web::Result<HttpResponse, ValidatorServerError> {
    let body = body.into_inner();

    let mut conn = redis.check_out(PoolDefault)
        .await
        .unwrap();

    // Verify
    if conn.exists(&body.id).await.unwrap() { return Ok(HttpResponse::Accepted().finish()); };
    let current_block = conn.get::<_, u128>(&body.id).await.unwrap();

    if body.block < (current_block - 5) || body.block > (current_block + 5) {
        return Ok(HttpResponse::BadRequest().finish());
    }
    
    if !verify_body(&body) {
        return Ok(HttpResponse::BadRequest().finish());
    };

    // Sign
    let sig = sign_body(body.id.as_str(), BUNDLER_ADDRESS.as_str())
        .await;

    // Add to db
    let current_epoch = conn.get::<_, i64>("validator:epoch:current")
        .await
        .unwrap();
        
    let new_transaction = NewTransaction {
        id: body.id,
        epoch: current_epoch,
        block_promised: i64::try_from(body.block).unwrap(),
        block_actual: None,
        signature: sig.clone(),
        validated: false,
    };

    actix_rt::task::spawn_blocking(move || {
        let c = db.get().unwrap();
        diesel::insert_into(transactions)
            .values::<NewTransaction>(new_transaction)
            .execute(&c)
    }).await??;
   

    Ok(HttpResponse::Ok()
        .insert_header(("Content-Type", "application/octet-stream"))
        .body(sig))
}

fn verify_body(body: &UnsignedBody) -> bool {
    let block = body.block.to_string()
        .as_bytes()
        .to_vec();

    let tx_id = body.id.as_bytes().to_vec();

    let message = deep_hash_sync(DeepHashChunk::Chunks(vec![
        DeepHashChunk::Chunk(BUNDLR_AS_BUFFER.into()),
        DeepHashChunk::Chunk(ONE_AS_BUFFER.into()),
        DeepHashChunk::Chunk(tx_id.into()),
        DeepHashChunk::Chunk(block.into())
    ])).unwrap();


    lazy_static! {
        static ref PUBLIC: PKey<Public> = {
            let jwk = JWK {
                kty: "RSA",
                e: "AQAB",
                n: BASE64URL.encode(std::env::var("BUNDLER_PUBLIC").unwrap().as_bytes())
            };

            let p = serde_json::to_string(&jwk).unwrap();
            let key: JsonWebKey = p.parse().unwrap();
            
            PKey::public_key_from_der(key.key.to_der().as_slice()).unwrap()
        };
    };

    dbg!(body.signature.clone());

    let sig = BASE64URL_NOPAD.decode(body.signature.as_bytes()).unwrap();
    
    let mut verifier = sign::Verifier::new(MessageDigest::sha256(), &PUBLIC).unwrap();
    verifier.set_rsa_padding(Padding::PKCS1_PSS).unwrap();
    verifier.update(&message).unwrap();
    verifier.verify(&sig).unwrap_or(false)
}

async fn sign_body(tx_id: &str, bundler_address: &str) -> Vec<u8> {
    let message = deep_hash(DeepHashChunk::Chunks(vec![
        DeepHashChunk::Chunk(VALIDATOR_AS_BUFFER.into()),
        DeepHashChunk::Chunk(ONE_AS_BUFFER.into()),
        DeepHashChunk::Chunk(BASE64URL_NOPAD.decode(tx_id.as_bytes()).unwrap().into()),
        DeepHashChunk::Chunk(BASE64URL_NOPAD.decode(bundler_address.as_bytes()).unwrap().into())
    ]))
        .await.unwrap();

    lazy_static! {
        static ref KEY: PKey<Private> = {
            let file: String = String::from_utf8(include_bytes!("../../../wallet.json").to_vec()).unwrap();
            let key: JsonWebKey = file.parse().unwrap();
            let pem = key.key.to_pem();
            PKey::private_key_from_pem(pem.as_bytes()).unwrap()
        };
    };

    let mut signer = sign::Signer::new(MessageDigest::sha256(), &KEY).unwrap();
    signer.set_rsa_padding(Padding::PKCS1_PSS).unwrap();
    signer.update(&message).unwrap();
    let mut sig = vec![0;256];
    signer.sign(&mut sig).unwrap();

    sig
}


#[cfg(test)]
mod tests {
    use super::verify_body;
    use super::UnsignedBody;

    #[test]
    fn test_sign_and_verify() {
        dotenv::dotenv();

        let body = UnsignedBody {
            id: "dtdOmHZMOtGb2C0zLqLBUABrONDZ5rzRh9NengT1-Zk".into(),
            signature: "W3s8FiKy96mgZ_QJll3XCLJBkFadtX4Oaky_GrCxg4kyi77WBLxXF1DjANBFYmSORdkM3b1lOlu-bcKnEMtAfIeWeEXtUw2uMCsFJIUSV1VkxKQRhmBRFy1xLC5TvZ4RxV_QOPCpPNQxbcCn9jF5mzvS27PBnJI6Zp06shlILGKw_zdNhUeYu4iWpkTTamKUzHrRO3XKUVwWhsHvZBSI_wv232x6X_2tPF4yIeveZJm5DUcCZZj0SZ8Rr6STLURzkWODPmBtMJMbZn3AblGMvrzBf14JuCUQqRiCJDNXwl-ICumwgQSDhFWoZjaYo5xTWzqdAsgSem39wmPEKFmli3nBDIZ49BuM12wCf6cDtD7Pua9rqYoR991clkv6H2xRbRAQl12xwvbKLvwHZe3PCcdI5sfcNLbhyVP3ZQ_WiD4B2gs9CDzc_1pYDVIwqr-Lfi_nQQXQwMEcSHCoP7lZlCnCPaXO_hzi3sBhdmhZJWK7oaqSl5eiWSFAnDWOY4Mhj95gPV7wUoIes41bNqSn7f_Ql614c3lbiALKhrh5O3izgK2-cuqHc4yKsFCj5ksWSQCroLeZnA5w5NmLP5lNyfTbSyTkbeHJigxaTgNihMdIFExmbmHd4R6-3lMVhvhyHLX4QypKYCoHanupCBQITz-RTRXmIvSJrKZFkWtA_dM".into(),
            block: 500,
        };

        assert!(verify_body(&body));
    }
}