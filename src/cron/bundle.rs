extern crate diesel;

use super::error::ValidatorCronError;
use super::slasher::vote_slash;
use super::transactions::get_transactions;
use crate::cron::arweave::arweave::Arweave;
use crate::database::models::{NewBundle, NewTransaction};
use crate::database::queries::*;
use crate::types::Validator;
use awc::Client;
use bundlr_sdk::deep_hash_sync::{deep_hash_sync, ONE_AS_BUFFER};
use bundlr_sdk::JWK;
use bundlr_sdk::{deep_hash::DeepHashChunk, verify::file::verify_file_bundle};
use data_encoding::BASE64URL_NOPAD;
use jsonwebkey::JsonWebKey;
use lazy_static::lazy_static;
use openssl::hash::MessageDigest;
use openssl::pkey::{PKey, Public};
use openssl::rsa::Padding;
use openssl::sign;
use paris::{error, info};
use serde::{Deserialize, Serialize};

#[derive(Default)]
pub struct Bundler {
    pub address: String,
    pub url: String,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct TxReceipt {
    block: i64,
    tx_id: String,
    signature: String,
}

pub async fn get_bundler() -> Result<Bundler, ValidatorCronError> {
    Ok(Bundler {
        address: "OXcT1sVRSA5eGwt2k6Yuz8-3e3g9WJi5uSE99CWqsBs".to_string(),
        url: "https://node1.bundlr.network/".to_string(),
    })
}

pub async fn validate_bundler(bundler: Bundler) -> Result<(), ValidatorCronError> {
    let arweave = Arweave::new(80, String::from("arweave.net"), String::from("http"));
    let txs_req = arweave
        .get_latest_transactions(&bundler.address, Some(50), None)
        .await;

    if let Err(r) = txs_req {
        error!(
            "Error occurred while getting txs from bundler address: \n {}. Error: {}",
            bundler.address, r
        );
        return Err(ValidatorCronError::TxsFromAddressNotFound);
    }

    let txs_req = &txs_req.unwrap().0;
    for bundle_tx in txs_req {
        // TODO: Check seeded [?]
        // TODO: Download bundle [?]
        let current_block = bundle_tx.block.as_ref().map(|b| b.height);

        if current_block.is_none() {
            info!(
                "Bundle {} not included in any block, moving on ...",
                &bundle_tx.id
            );
            continue;
        } else {
            info!(
                "Bundle {} included in block {}",
                &bundle_tx.id,
                current_block.unwrap()
            );
            let is_bundle_present = get_bundle(&bundle_tx.id).is_ok();

            if !is_bundle_present {
                match insert_bundle_in_db(NewBundle {
                    id: bundle_tx.id.clone(),
                    owner_address: Some(bundler.address.clone()),
                    block_height: current_block.unwrap(),
                }) {
                    Ok(()) => info!("Bundle {} successfully stored", &bundle_tx.id),
                    Err(err) => error!("Error when storing bundle {} : {}", &bundle_tx.id, err),
                }
            }
        }

        let file_path = arweave.get_tx_data(&bundle_tx.id).await;
        if file_path.is_ok() {
            info!("Verifying file: {}", &file_path.as_ref().unwrap());
            let path_str = file_path.unwrap().to_string();
            let bundle_txs = match verify_file_bundle(path_str.clone()).await {
                Err(r) => {
                    error!("{}", r);
                    Vec::new()
                }
                Ok(v) => v,
            };

            for bundle_tx in bundle_txs {
                info!("Verifying bundle_tx: {}", &bundle_tx.tx_id);

                let tx = get_tx(&bundle_tx.tx_id).await;
                let mut tx_receipt: Option<TxReceipt> = None;
                if tx.is_err() {
                    let peer_tx = tx_exists_on_peers(&bundle_tx.tx_id).await;
                    if peer_tx.is_ok() {
                        tx_receipt = Some(peer_tx.unwrap());
                    }
                } else {
                    let tx = tx.unwrap();
                    tx_receipt = Some(TxReceipt {
                        block: tx.block_promised,
                        tx_id: tx.id,
                        signature: match std::str::from_utf8(&tx.signature.to_vec()) {
                            Ok(v) => v.to_string(),
                            Err(e) => panic!("Invalid UTF-8 seq: {}", e),
                        },
                    });
                }

                info!("Tx receipt: {:?}", &tx_receipt);
                /*
                let tx_is_ok = verify_tx_receipt(&tx_receipt);
                if tx_is_ok.unwrap() {
                    if tx_receipt.block <= current_block.unwrap() {
                        insert_tx_in_db(&NewTransaction {
                            id: tx_receipt.tx_id.clone(),
                            epoch: 0, // TODO: implement epoch correctly
                            block_promised: tx_receipt.block,
                            block_actual: current_block,
                            signature: tx_receipt.signature.as_bytes().to_vec(),
                            validated: true,
                            bundle_id: Some(bundle_tx.tx_id.clone()),
                            sent_to_leader: false,
                        });
                    } else {
                        // TODO: vote slash
                    }
                }
                */
            }

            match std::fs::remove_file(path_str.clone()) {
                Ok(_r) => info!("Successfully deleted {}", path_str),
                Err(err) => error!("Error deleting file {} : {}", path_str, err),
            }
        }
    }

    // If no - sad - OK

    // If yes - check that block_seeded == block_expected -

    // If valid - return

    // If not - vote to slash... once vote is confirmed then tell all peers to check

    Ok(())
}

async fn tx_exists_on_peers(tx_id: &str) -> Result<TxReceipt, ValidatorCronError> {
    let client = Client::default();
    let validator_peers = Vec::<Validator>::new();
    for peer in validator_peers {
        let response = client
            .get(format!("{}/tx/{}", peer.url, tx_id))
            .send()
            .await;

        if let Err(r) = response {
            error!("Error occurred while getting tx from peer - {}", r);
            continue;
        }

        let mut response = response.unwrap();

        if response.status().is_success() {
            return Ok(response.json().await.unwrap());
        }
    }

    Err(ValidatorCronError::TxNotFound)
}

fn verify_tx_receipt(tx_receipt: &TxReceipt) -> std::io::Result<bool> {
    pub const BUNDLR_AS_BUFFER: &[u8] = "Bundlr".as_bytes();

    let block = tx_receipt.block.to_string().as_bytes().to_vec();

    let tx_id = tx_receipt.tx_id.as_bytes().to_vec();

    let message = deep_hash_sync(DeepHashChunk::Chunks(vec![
        DeepHashChunk::Chunk(BUNDLR_AS_BUFFER.into()),
        DeepHashChunk::Chunk(ONE_AS_BUFFER.into()),
        DeepHashChunk::Chunk(tx_id.into()),
        DeepHashChunk::Chunk(block.into()),
    ]))
    .unwrap();

    lazy_static! {
        static ref PUBLIC: PKey<Public> = {
            let jwk = JWK {
                kty: "RSA",
                e: "AQAB",
                n: std::env::var("BUNDLER_PUBLIC").unwrap(),
            };

            let p = serde_json::to_string(&jwk).unwrap();
            let key: JsonWebKey = p.parse().unwrap();

            PKey::public_key_from_der(key.key.to_der().as_slice()).unwrap()
        };
    };

    let sig = BASE64URL_NOPAD
        .decode(tx_receipt.signature.as_bytes())
        .unwrap();

    let mut verifier = sign::Verifier::new(MessageDigest::sha256(), &PUBLIC).unwrap();
    verifier.set_rsa_padding(Padding::PKCS1_PSS).unwrap();
    verifier.update(&message).unwrap();
    Ok(verifier.verify(&sig).unwrap_or(false))
}

pub async fn validate_transactions(bundler: Bundler) -> Result<(), ValidatorCronError> {
    let res = get_transactions(&bundler, Some(100), None).await;
    let txs = match res {
        Ok(r) => r.0,
        Err(_) => Vec::new(),
    };

    for tx in txs {
        // TODO: validate transacitons
        let block_ok = tx.current_block < tx.expected_block;

        if block_ok {
            let _res = vote_slash(&bundler);
        }
    }

    Ok(())
}
