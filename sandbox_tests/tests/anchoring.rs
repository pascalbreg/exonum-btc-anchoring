#[macro_use]
extern crate exonum;
extern crate sandbox;
extern crate anchoring_btc_service;
#[macro_use]
extern crate anchoring_btc_sandbox;
extern crate serde;
extern crate serde_json;
extern crate bitcoin;
extern crate bitcoinrpc;
extern crate secp256k1;
extern crate blockchain_explorer;
#[macro_use]
extern crate log;

use std::ops::Deref;

use bitcoin::util::base58::ToBase58;
use bitcoin::blockdata::script::Script;
use bitcoin::blockdata::transaction::SigHashType;
use bitcoin::network::constants::Network;

use exonum::crypto::{HexValue, Hash};
use exonum::messages::Message;
use sandbox::sandbox_tests_helper::{SandboxState, add_one_height_with_transactions};

use anchoring_btc_service::details::sandbox::Request;
use anchoring_btc_service::details::btc::transactions::{TransactionBuilder, AnchoringTx,
                                                        FundingTx, verify_tx_input};
use anchoring_btc_service::blockchain::dto::MsgAnchoringSignature;
use anchoring_btc_sandbox::{RpcError, anchoring_sandbox};
use anchoring_btc_sandbox::helpers::*;
use anchoring_btc_sandbox::secp256k1_hack::sign_tx_input_with_nonce;

// We anchor first block
// problems: None
// result: success
#[test]
fn test_anchoring_first_block() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();
    anchor_first_block(&sandbox, &client, &sandbox_state, &mut anchoring_state);
}

// We wait until `funding_tx` have got enough confirmations.
// problems: None
// result: success
#[test]
fn test_anchoring_funding_tx_waiting() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();

    let funding_tx = anchoring_state.common.funding_tx.clone();

    client.expect(vec![gen_confirmations_request(funding_tx.clone(), 0)]);
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[]);
    // Resend funding_tx if we lost it
    client.expect(vec![request! {
                           method: "getrawtransaction",
                           params: [&funding_tx.txid(), 1],
                           error: RpcError::NoInformation("Unable to find tx".to_string()),
                       },
                       request! {
                           method: "sendrawtransaction",
                           params: [&funding_tx.to_hex()]
                       }]);
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[]);

    client.expect(vec![gen_confirmations_request(funding_tx.clone(), 0)]);
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[]);
}

// We anchor first block and receive lect
// problems: None
// result: success
#[test]
fn test_anchoring_update_lect_normal() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();

    anchor_first_block(&sandbox, &client, &sandbox_state, &mut anchoring_state);
    anchor_first_block_lect_normal(&sandbox, &client, &sandbox_state, &mut anchoring_state);
}

// We anchor first block and receive lect with different but correct signatures
// problems: lect with a different signature set
// result: success with a new lect
#[test]
fn test_anchoring_update_lect_different() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();
    anchor_first_block_lect_different(&sandbox, &client, &sandbox_state, &mut anchoring_state);
}

// We anchor first block and lose anchoring transaction
// problems: anchoring transaction is lost
// result: we have lost anchoring transaction
#[test]
fn test_anchoring_first_block_lect_lost() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();
    anchor_first_block_lect_lost(&sandbox, &client, &sandbox_state, &mut anchoring_state);
    assert_eq!(anchoring_state.latest_anchored_tx, None);
}

// We anchor second block after successfuly anchored first
// problems: none
// result: success
#[test]
fn test_anchoring_second_block_normal() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();
    anchor_first_block(&sandbox, &client, &sandbox_state, &mut anchoring_state);
    anchor_first_block_lect_normal(&sandbox, &client, &sandbox_state, &mut anchoring_state);
    anchor_second_block_normal(&sandbox, &client, &sandbox_state, &mut anchoring_state);
}

// We anchor second block after successfuly anchored first with additional funds
// problems: none
// result: success
#[test]
fn test_anchoring_second_block_additional_funds() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();

    let (_, anchoring_addr) = anchoring_state.common.redeem_script();

    anchor_first_block(&sandbox, &client, &sandbox_state, &mut anchoring_state);
    anchor_first_block_lect_normal(&sandbox, &client, &sandbox_state, &mut anchoring_state);
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[]);
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[]);
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[]);

    let funds = anchoring_state.common.funding_tx.clone();
    client.expect(vec![request! {
        method: "listunspent",
        params: [0, 9999999, [&anchoring_addr.to_base58check()]],
        response: [
            {
                "txid": "fea0a60f7146e7facf5bb382b80dafb762175bf0d4b6ac4e59c09cd4214d1491",
                "vout": 0,
                "address": &anchoring_addr.to_base58check(),
                "account": "multisig",
                "scriptPubKey": "a914499d997314d6e55e49293b50d8dfb78bb9c958ab87",
                "amount": 0.00010000,
                "confirmations": 1,
                "spendable": false,
                "solvable": false
            },
            {
                "txid": &funds.txid(),
                "vout": 0,
                "address": &anchoring_addr.to_base58check(),
                "account": "multisig",
                "scriptPubKey": "a914499d997314d6e55e49293b50d8dfb78bb9c958ab87",
                "amount": 0.00010000,
                "confirmations": 75,
                "spendable": false,
                "solvable": false
            }
        ]
    }]);
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[]);

    let (_, signatures) = anchoring_state
        .gen_anchoring_tx_with_signatures(&sandbox,
                                          10,
                                          block_hash_on_height(&sandbox, 10),
                                          &[funds],
                                          None,
                                          &anchoring_addr);

    sandbox.broadcast(signatures[0].clone());
    sandbox.broadcast(signatures[1].clone());

    let anchored_tx = anchoring_state.latest_anchored_tx();
    client.expect(vec![request! {
                           method: "getrawtransaction",
                           params: [&anchored_tx.txid(), 1],
                           error: RpcError::NoInformation("Unable to find tx".to_string()),
                       },
                       request! {
                           method: "sendrawtransaction",
                           params: [&anchored_tx.to_hex()]
                       }]);

    add_one_height_with_transactions(&sandbox, &sandbox_state, &signatures);
    sandbox.broadcast(gen_service_tx_lect(&sandbox, 0, &anchored_tx, 2));
}

// We anchor second block after successfuly anchored first
// problems: second anchoring tx is lost
// result: we have lost anchoring tx
#[test]
fn test_anchoring_second_block_lect_lost() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();
    let (_, anchoring_addr) = anchoring_state.common.redeem_script();

    anchor_first_block(&sandbox, &client, &sandbox_state, &mut anchoring_state);
    anchor_first_block_lect_normal(&sandbox, &client, &sandbox_state, &mut anchoring_state);

    let prev_anchored_tx = anchoring_state.latest_anchored_tx().clone();
    let prev_tx_signatures = anchoring_state.latest_anchored_tx_signatures().to_vec();

    anchor_second_block_normal(&sandbox, &client, &sandbox_state, &mut anchoring_state);
    for _ in 0..5 {
        add_one_height_with_transactions(&sandbox, &sandbox_state, &[]);
    }

    client.expect(vec![request! {
        method: "listunspent",
            params: [0, 9999999, [&anchoring_addr.to_base58check()]],
            response: [
                {
                    "txid": &prev_anchored_tx.txid(),
                    "vout": 0,
                    "address": &anchoring_addr.to_base58check(),
                    "account": "multisig",
                    "scriptPubKey": "a914499d997314d6e55e49293b50d8dfb78bb9c958ab87",
                    "amount": 0.00010000,
                    "confirmations": 0,
                    "spendable": false,
                    "solvable": false
                }
            ]
        },
                       request! {
             method: "getrawtransaction",
             params: [&prev_anchored_tx.txid(), 0],
             response: &prev_anchored_tx.to_hex()
        }]);

    add_one_height_with_transactions(&sandbox, &sandbox_state, &[]);

    let txs = (0..4)
        .map(|id| gen_service_tx_lect(&sandbox, id, &prev_anchored_tx, 3))
        .collect::<Vec<_>>();
    sandbox.broadcast(txs[0].clone());

    // Trying to resend lost lect tx
    client.expect(vec![request! {
                        method: "listunspent",
                        params: [0, 9999999, [&anchoring_addr.to_base58check()]],
                        response: [
                            {
                                "txid": &prev_anchored_tx.txid(),
                                "vout": 0,
                                "address": &anchoring_addr.to_base58check(),
                                "account": "multisig",
                                "scriptPubKey": "a914499d997314d6e55e49293b50d8dfb78bb9c958ab87",
                                "amount": 0.00010000,
                                "confirmations": 0,
                                "spendable": false,
                                "solvable": false
                            }
                        ]
                    }]);
    add_one_height_with_transactions(&sandbox, &sandbox_state, &txs);

    anchoring_state.latest_anchored_tx = Some((prev_anchored_tx, prev_tx_signatures));
}

// We find lect, whose prev_hash is not known
// problems: prev_hash is unknown
// result: we unroll chain to funding_tx up to funding_tx and update lect
#[test]
fn test_anchoring_find_lect_chain_normal() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();
    anchor_first_block(&sandbox, &client, &sandbox_state, &mut anchoring_state);

    // Just add few heights
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[]);
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[]);

    let (_, anchoring_addr) = anchoring_state.common.redeem_script();
    let anchored_txs = (1..3)
        .map(|height| {
            anchoring_state.gen_anchoring_tx_with_signatures(&sandbox,
                                                             height,
                                                             block_hash_on_height(&sandbox,
                                                                                  height),
                                                             &[],
                                                             None,
                                                             &anchoring_addr);
            anchoring_state.latest_anchored_tx().clone()
        })
        .collect::<Vec<_>>();
    let current_anchored_tx = anchored_txs.last().unwrap();

    let request = {
        let mut request = Vec::new();

        request.push(request! {
            method: "listunspent",
            params: [0, 9999999, [&anchoring_addr.to_base58check()]],
            response: [
                {
                    "txid": &current_anchored_tx.txid(),
                    "vout": 0,
                    "address": &anchoring_addr.to_base58check(),
                    "account": "multisig",
                    "scriptPubKey": "a914499d997314d6e55e49293b50d8dfb78bb9c958ab87",
                    "amount": 0.00010000,
                    "confirmations": 0,
                    "spendable": false,
                    "solvable": false
                }
            ]
        });
        for tx in anchored_txs.iter().rev() {
            request.push(request! {
                method: "getrawtransaction",
                params: [&tx.txid(), 0],
                response: &tx.to_hex()
            });
        }
        request
    };
    client.expect(request);
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[]);

    let txs = (0..4)
        .map(|idx| gen_service_tx_lect(&sandbox, idx, &current_anchored_tx, 2))
        .collect::<Vec<_>>();
    sandbox.broadcast(txs[0].clone());
}

// We find lect, whose prev_hash is not known
// problems: prev_hash is unknown, chain has wrong prev_hashes
// result: we unroll chain to funding_tx up to weird tx and discard lect
#[test]
fn test_anchoring_find_lect_chain_wrong() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();
    anchor_first_block(&sandbox, &client, &sandbox_state, &mut anchoring_state);

    // Just add few heights
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[]);
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[]);

    let (_, anchoring_addr) = anchoring_state.common.redeem_script();
    let anchored_txs = {

        let mut tx = AnchoringTx::from_hex("0100000001c13d4c739390c799344fa89fb701add04e5ccaf3d580\
            e4d4379c4b897e3a2266000000006b483045022100ff88211040a8a95a42ca8520749c1b2b4024ce07b3ed\
            1b51da8bb90ef77dbe5d022034b34ef638d23ef0ea532e2c84a8816cb32021112d4bcf1457b4e2c149d1b8\
            3f01210250749a68b12a93c2cca6f86a9a9c9ba37f5191e85334c340856209a17cca349afeffffff024042\
            0f000000000017a914180d8e6b0ad7f63177e943752c278294709425bd872908da0b000000001976a914de\
            e9f9433b3f2d24cbd833f83a41e4c1235efa3f88acd6ac1000")
                .unwrap();
        let mut txs = vec![tx.clone()];
        for height in 1..4 {
            tx = TransactionBuilder::with_prev_tx(&tx, 0)
                .fee(100)
                .payload(height, block_hash_on_height(&sandbox, height))
                .send_to(anchoring_addr.clone())
                .into_transaction()
                .unwrap();
            txs.push(tx.clone());
        }
        anchoring_state.latest_anchored_tx = Some((tx, vec![]));
        txs
    };
    let current_anchored_tx = anchored_txs.last().unwrap();

    let request = {
        let mut request = Vec::new();

        request.push(request! {
            method: "listunspent",
            params: [0, 9999999, [&anchoring_addr.to_base58check()]],
            response: [
                {
                    "txid": &current_anchored_tx.txid(),
                    "vout": 0,
                    "address": &anchoring_addr.to_base58check(),
                    "account": "multisig",
                    "scriptPubKey": "a914499d997314d6e55e49293b50d8dfb78bb9c958ab87",
                    "amount": 0.00010000,
                    "confirmations": 0,
                    "spendable": false,
                    "solvable": false
                },
            ]
        });
        for tx in anchored_txs.iter().rev() {
            request.push(request! {
                method: "getrawtransaction",
                params: [&tx.txid(), 0],
                response: &tx.to_hex()
            });
        }
        request
    };
    client.expect(request);
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[]);
}

// We received lect message with correct content
// problems: None
// result: we appect it
#[test]
fn test_anchoring_lect_correct_validator() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();
    anchor_first_block(&sandbox, &client, &sandbox_state, &mut anchoring_state);

    let msg_lect =
        gen_service_tx_lect_wrong(&sandbox, 0, 0, anchoring_state.latest_anchored_tx(), 2);
    // Commit `msg_lect` into blockchain
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[msg_lect]);
    // Ensure that service accepts it
    let lects_after = dump_lects(&sandbox, 0);
    assert_eq!(lects_after.last().unwrap(),
               anchoring_state.latest_anchored_tx().deref());
}

// We received lect message with different validator id
// problems: None
// result: we ignore it
#[test]
fn test_anchoring_lect_wrong_validator() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();
    anchor_first_block(&sandbox, &client, &sandbox_state, &mut anchoring_state);

    let msg_lect_wrong =
        gen_service_tx_lect_wrong(&sandbox, 2, 0, anchoring_state.latest_anchored_tx(), 2);

    let lects_before = dump_lects(&sandbox, 0);
    // Commit `msg_lect_wrong` into blockchain
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[msg_lect_wrong]);
    // Ensure that service ignores it
    let lects_after = dump_lects(&sandbox, 0);
    assert_eq!(lects_after, lects_before);
}

// We received lect message with nonexistent validator id
// problems: None
// result: we ignore it
#[test]
fn test_anchoring_lect_nonexistent_validator() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();
    anchor_first_block(&sandbox, &client, &sandbox_state, &mut anchoring_state);

    let msg_lect_wrong =
        gen_service_tx_lect_wrong(&sandbox, 2, 1000, anchoring_state.latest_anchored_tx(), 2);

    let lects_before = dump_lects(&sandbox, 2);
    // Commit `msg_lect_wrong` into blockchain
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[msg_lect_wrong]);
    // Ensure that service ignores it
    let lects_after = dump_lects(&sandbox, 0);
    assert_eq!(lects_after, lects_before);
}

// We received signature message with wrong sign
// problems: None
// result: we ignore it
#[test]
fn test_anchoring_signature_wrong_validator() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();

    anchor_first_block(&sandbox, &client, &sandbox_state, &mut anchoring_state);
    anchor_first_block_lect_normal(&sandbox, &client, &sandbox_state, &mut anchoring_state);

    let signatures = anchoring_state.latest_anchored_tx_signatures();
    let tx = {
        let mut tx = anchoring_state.latest_anchored_tx().clone();
        tx.0.input[0].script_sig = Script::new();
        tx
    };

    let msg_signature_wrong = MsgAnchoringSignature::new(&sandbox.p(1),
                                                         1,
                                                         tx.clone(),
                                                         0,
                                                         signatures[0].signature(),
                                                         sandbox.s(1));


    let signs_before = dump_signatures(&sandbox, &tx.id());
    // Commit `msg_signature_wrong` into blockchain
    add_one_height_with_transactions(&sandbox,
                                     &sandbox_state,
                                     &[msg_signature_wrong.raw().clone()]);
    // Ensure that service ignores it
    let signs_after = dump_signatures(&sandbox, &tx.id());
    assert_eq!(signs_before, signs_after);
}

// We received correct signature message with nonexistent id
// problems: None
// result: we add signature
#[test]
fn test_anchoring_signature_nonexistent_tx() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();

    anchor_first_block(&sandbox, &client, &sandbox_state, &mut anchoring_state);
    anchor_first_block_lect_normal(&sandbox, &client, &sandbox_state, &mut anchoring_state);

    let (redeem_script, addr) = anchoring_state.common.redeem_script();
    let tx = TransactionBuilder::with_prev_tx(anchoring_state.latest_anchored_tx(), 0)
        .fee(100)
        .payload(0, block_hash_on_height(&sandbox, 0))
        .send_to(addr.clone())
        .into_transaction()
        .unwrap();
    let signature = tx.sign_input(&redeem_script, 0, &anchoring_state.priv_keys(&addr)[1]);
    let msg_sign = MsgAnchoringSignature::new(&sandbox.p(1),
                                              1,
                                              tx.clone(),
                                              0,
                                              signature.as_ref(),
                                              sandbox.s(1));


    let signs_before = dump_signatures(&sandbox, &tx.id());
    // Commit `msg_sign` into blockchain
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[msg_sign.raw().clone()]);
    // Ensure that service adds it
    let signs_after = dump_signatures(&sandbox, &tx.id());
    assert!(signs_before.is_empty());
    assert_eq!(signs_after[0], msg_sign);
}

// We received correct signature message with incorrect payload
// problems: None
// result: we ignore it
#[test]
fn test_anchoring_signature_incorrect_payload() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();

    anchor_first_block(&sandbox, &client, &sandbox_state, &mut anchoring_state);
    anchor_first_block_lect_normal(&sandbox, &client, &sandbox_state, &mut anchoring_state);

    let (redeem_script, addr) = anchoring_state.common.redeem_script();
    let tx = TransactionBuilder::with_prev_tx(anchoring_state.latest_anchored_tx(), 0)
        .fee(100)
        .payload(0, Hash::zero())
        .send_to(addr.clone())
        .into_transaction()
        .unwrap();
    let signature = tx.sign_input(&redeem_script, 0, &anchoring_state.priv_keys(&addr)[1]);
    let msg_sign = MsgAnchoringSignature::new(&sandbox.p(1),
                                              1,
                                              tx.clone(),
                                              0,
                                              signature.as_ref(),
                                              sandbox.s(1));


    let signatures_before = dump_signatures(&sandbox, &tx.id());
    // Commit `msg_sign` into blockchain
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[msg_sign.raw().clone()]);
    // Ensure that service ignores it
    let signatures_after = dump_signatures(&sandbox, &tx.id());
    assert!(signatures_before.is_empty());
    assert!(signatures_after.is_empty());
}

// We received correct lect with the current funding_tx
// problems: None
// result: we add it
#[test]
fn test_anchoring_lect_funding_tx() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();

    anchor_first_block(&sandbox, &client, &sandbox_state, &mut anchoring_state);
    anchor_first_block_lect_normal(&sandbox, &client, &sandbox_state, &mut anchoring_state);

    let tx = anchoring_state.common.funding_tx;
    let msg_lect = gen_service_tx_lect(&sandbox, 0, &tx, 2);
    let lects_before = dump_lects(&sandbox, 0);
    // Commit `msg_lect` into blockchain
    client.expect(vec![gen_confirmations_request(tx.clone(), 50)]);
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[msg_lect.raw().clone()]);
    // Ensure that service accepts it
    let lects_after = dump_lects(&sandbox, 0);
    assert_eq!(lects_before.len(), 2);
    assert_eq!(lects_after[2], tx.0);
}

// We received correct lect with the incorrect funding_tx
// problems: None
// result: we ignore it
#[test]
fn test_anchoring_lect_incorrect_funding_tx() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();

    anchor_first_block(&sandbox, &client, &sandbox_state, &mut anchoring_state);
    anchor_first_block_lect_normal(&sandbox, &client, &sandbox_state, &mut anchoring_state);

    // Create `p2sh` transaction for unknown multisig address
    // For details see link be470954627bfbde664b5adc2bbb98280e8491918cf4a678d16cab13e8a9865b
    // transaction in https://www.blocktrail.com/tBTC/tx
    let tx = FundingTx::from_hex("02000000017ed7e5c5ebec6c7d3d012f543b0656216ccc3710f4272e0b663176\
        598a9271da010000006a47304402206746eab4ce2a720307686b3b255e8fed99dee6da01575d67394c7b7e0953\
        93a102206fbb34a95c139ad217c767bf2bc1db5521af2407ea12d47b3a9e9babfba58df9012102b3aac66108bf\
        a3eee4075c6adee8fd417ca535cfdbe1637be418b7ab92cc5346feffffff02a00f00000000000017a91424f0d3\
        4abfec4f4cb19d942202529b8653a0a58d870c170f0c000000001976a9147bb8844ee71cbd2bc735411f4e2997\
        1f697fed0a88ac81131100")
            .unwrap();
    let msg_lect = gen_service_tx_lect(&sandbox, 0, &tx, 2);
    let lects_before = dump_lects(&sandbox, 0);
    // Commit `msg_lect` into blockchain
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[msg_lect.raw().clone()]);
    // Ensure that service ignores it
    let lects_after = dump_lects(&sandbox, 0);
    assert_eq!(lects_before, lects_after);
}

// We received correct lect with the incorrect anchoring payload
// problems: None
// result: we ignore it
#[test]
fn test_anchoring_lect_incorrect_anchoring_payload() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();

    anchor_first_block(&sandbox, &client, &sandbox_state, &mut anchoring_state);
    anchor_first_block_lect_normal(&sandbox, &client, &sandbox_state, &mut anchoring_state);

    let tx = TransactionBuilder::with_prev_tx(&anchoring_state.common.funding_tx, 0)
        .fee(1000)
        .payload(0, Hash::zero())
        .send_to(anchoring_state.common.redeem_script().1)
        .into_transaction()
        .unwrap();
    let msg_lect = gen_service_tx_lect(&sandbox, 0, &tx, 2);
    let lects_before = dump_lects(&sandbox, 0);
    // Commit `msg_lect` into blockchain
    add_one_height_with_transactions(&sandbox, &sandbox_state, &[msg_lect.raw().clone()]);
    // Ensure that service ignores it
    let lects_after = dump_lects(&sandbox, 0);
    assert_eq!(lects_before, lects_after);
}

// We received signature message with wrong sign
// problems: None
// result: we ignore it
#[test]
fn test_anchoring_signature_nonexistent_validator() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();

    anchor_first_block(&sandbox, &client, &sandbox_state, &mut anchoring_state);
    anchor_first_block_lect_normal(&sandbox, &client, &sandbox_state, &mut anchoring_state);

    let signatures = anchoring_state.latest_anchored_tx_signatures();
    let tx = {
        let mut tx = anchoring_state.latest_anchored_tx().clone();
        tx.0.input[0].script_sig = Script::new();
        tx
    };

    let msg_signature_wrong = MsgAnchoringSignature::new(&sandbox.p(1),
                                                         1000,
                                                         tx.clone(),
                                                         0,
                                                         signatures[0].signature(),
                                                         sandbox.s(1));


    let signs_before = dump_signatures(&sandbox, &tx.id());
    // Commit `msg_signature_wrong` into blockchain
    add_one_height_with_transactions(&sandbox,
                                     &sandbox_state,
                                     &[msg_signature_wrong.raw().clone()]);
    // Ensure that service ignores it
    let signs_after = dump_signatures(&sandbox, &tx.id());
    assert_eq!(signs_before, signs_after);
}

// We received signature message with correct input but different signature
// problems: None
// result: we ignore it
#[test]
fn test_anchoring_signature_input_with_different_correct_signature() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();

    anchor_first_block(&sandbox, &client, &sandbox_state, &mut anchoring_state);
    anchor_first_block_lect_normal(&sandbox, &client, &sandbox_state, &mut anchoring_state);

    let signature_msgs = anchoring_state.latest_anchored_tx_signatures();
    let tx = {
        let mut tx = anchoring_state.latest_anchored_tx().clone();
        tx.0.input[0].script_sig = Script::new();
        tx
    };

    let msg_signature_different = {
        let (redeem_script, addr) = anchoring_state.common.redeem_script();
        let pub_key = &anchoring_state.common.validators[1];
        let priv_key = &anchoring_state.priv_keys(&addr)[1];

        let mut different_signature =
            sign_tx_input_with_nonce(&tx, 0, &redeem_script, priv_key.secret_key(), 2);
        assert!(verify_tx_input(&tx,
                                0,
                                &redeem_script,
                                pub_key,
                                different_signature.as_ref()));

        different_signature.push(SigHashType::All.as_u32() as u8);
        assert!(different_signature != signature_msgs[1].signature());

        MsgAnchoringSignature::new(&sandbox.p(1),
                                   1,
                                   tx.clone(),
                                   0,
                                   different_signature.as_ref(),
                                   sandbox.s(1))
    };
    assert!(signature_msgs[1] != msg_signature_different);

    let signs_before = dump_signatures(&sandbox, &tx.id());
    // Commit `msg_signature_different` into blockchain
    add_one_height_with_transactions(&sandbox,
                                     &sandbox_state,
                                     &[msg_signature_different.raw().clone()]);
    // Ensure that service ignores it
    let signs_after = dump_signatures(&sandbox, &tx.id());
    assert_eq!(signs_before, signs_after);
}

// We received signature message with correct signature
// but signed by different validator
// problems: None
// result: we ignore it
#[test]
fn test_anchoring_signature_input_from_different_validator() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();

    anchor_first_block_without_other_signatures(&sandbox,
                                                &client,
                                                &sandbox_state,
                                                &mut anchoring_state);

    let signatures = anchoring_state.latest_anchored_tx_signatures();
    let tx = {
        let mut tx = anchoring_state.latest_anchored_tx().clone();
        tx.0.input[0].script_sig = Script::new();
        tx
    };

    let msg_signature_wrong = MsgAnchoringSignature::new(&sandbox.p(1),
                                                         2,
                                                         tx.clone(),
                                                         0,
                                                         signatures[2].signature(),
                                                         sandbox.s(1));

    let signs_before = dump_signatures(&sandbox, &tx.id());
    // Commit `msg_signature_different` into blockchain
    client.expect(vec![gen_confirmations_request(anchoring_state.common.funding_tx.clone(), 50)]);
    add_one_height_with_transactions(&sandbox,
                                     &sandbox_state,
                                     &[msg_signature_wrong.raw().clone()]);
    // Ensure that service ignores it
    let signs_after = dump_signatures(&sandbox, &tx.id());
    assert_eq!(signs_before, signs_after);
}

// We received signature message for anchoring tx with unknown output_address
// problems: None
// result: we ignore it
#[test]
fn test_anchoring_signature_unknown_output_address() {
    let _ = ::blockchain_explorer::helpers::init_logger();

    let (sandbox, client, mut anchoring_state) = anchoring_sandbox(&[]);
    let sandbox_state = SandboxState::new();

    anchor_first_block(&sandbox, &client, &sandbox_state, &mut anchoring_state);
    anchor_first_block_lect_normal(&sandbox, &client, &sandbox_state, &mut anchoring_state);

    let tx = {
        let (_, addr) = {
            let mut anchoring_cfg = anchoring_state.common.clone();
            anchoring_cfg.validators.swap(1, 2);
            anchoring_cfg.redeem_script()
        };

        TransactionBuilder::with_prev_tx(anchoring_state.latest_anchored_tx(), 0)
            .fee(1000)
            .payload(0, Hash::zero())
            .send_to(addr)
            .into_transaction()
            .unwrap()
    };
    let (redeem_script, addr) = anchoring_state.common.redeem_script();
    let priv_key = &anchoring_state.current_priv_keys()[0];
    let signature = tx.sign_input(&redeem_script, 0, &priv_key);

    assert!(tx.output_address(Network::Testnet) != addr);
    assert!(tx.verify_input(&redeem_script,
                            0,
                            &anchoring_state.common.validators[0],
                            &signature));

    let msg_signature_wrong =
        MsgAnchoringSignature::new(&sandbox.p(0), 0, tx.clone(), 0, &signature, sandbox.s(0));

    let signs_before = dump_signatures(&sandbox, &tx.id());
    // Commit `msg_signature_wrong` into blockchain
    add_one_height_with_transactions(&sandbox,
                                     &sandbox_state,
                                     &[msg_signature_wrong.raw().clone()]);
    // Ensure that service ignores it
    let signs_after = dump_signatures(&sandbox, &tx.id());
    assert_eq!(signs_before, signs_after);
}
