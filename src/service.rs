use std::sync::{Arc, Mutex, MutexGuard};
use std::collections::HashMap;

use bitcoinrpc::{MultiSig, Error as RpcError};
use serde_json::Value;
use serde_json::value::{ToJson, from_value};

use exonum::blockchain::{Service, Transaction, Schema, NodeState};
use exonum::storage::{StorageValue, List, View, Error as StorageError};
use exonum::crypto::{Hash, ToHex};
use exonum::messages::{RawTransaction, Message, FromRaw, Error as MessageError};
use exonum::node::Height;

use config::{AnchoringNodeConfig, AnchoringConfig};
use {BITCOIN_NETWORK, AnchoringTx, FundingTx, AnchoringRpc, RpcClient, BitcoinPrivateKey, HexValue};
use schema::{ANCHORING_SERVICE, AnchoringTransaction, AnchoringSchema, TxAnchoringUpdateLatest,
             TxAnchoringSignature};

pub struct AnchoringState {
    proposal_tx: Option<AnchoringTx>,
}

pub struct AnchoringService {
    cfg: AnchoringNodeConfig,
    genesis: AnchoringConfig,
    client: RpcClient,
    state: Arc<Mutex<AnchoringState>>,
}

pub enum LectKind {
    Some(AnchoringTx),
    Different,
    None,
}

impl AnchoringService {
    pub fn new(client: RpcClient,
               genesis: AnchoringConfig,
               cfg: AnchoringNodeConfig)
               -> AnchoringService {
        let state = AnchoringState { proposal_tx: None };

        AnchoringService {
            cfg: cfg,
            genesis: genesis,
            client: client,
            state: Arc::new(Mutex::new(state)),
        }
    }

    pub fn majority_count(&self, state: &NodeState) -> Result<usize, StorageError> {
        let (_, cfg) = self.actual_config(state)?;
        Ok(cfg.validators.len() * 2 / 3 + 1)
    }

    pub fn client(&self) -> &RpcClient {
        &self.client
    }

    pub fn service_state(&self) -> MutexGuard<AnchoringState> {
        self.state.lock().unwrap()
    }

    pub fn actual_config(&self,
                         state: &NodeState)
                         -> Result<(BitcoinPrivateKey, AnchoringConfig), StorageError> {
        let genesis: AnchoringConfig =
            AnchoringSchema::new(state.view()).current_anchoring_config()?;
        let redeem_script = genesis.redeem_script();
        let key = self.cfg.private_keys[&redeem_script.to_address(BITCOIN_NETWORK)].clone();
        Ok((key, genesis))
    }

    pub fn output_address(&self, state: &NodeState) -> MultiSig {
        let genesis: AnchoringConfig = from_value(state.service_config(self).clone()).unwrap();
        genesis.multisig()
    }

    //     pub fn update_config(&self, updated: AnchoringConfig) {
    //         let mut state = self.service_state();
    //         if state.genesis != updated {
    //             debug!("Update anchoring service config");
    //             state.genesis = updated;

    //             let redeem_script = updated.redeem_script();
    //             self.client.importaddress(&redeem_script.to_address(BITCOIN_NETWORK),
    //                                       "multisig",
    //                                       false,
    //                                       false);
    //         }
    //     }

    pub fn actual_payload(&self, state: &NodeState) -> Result<(Height, Hash), StorageError> {
        let schema = Schema::new(state.view());
        let (_, genesis) = self.actual_config(state)?;

        let height = genesis.nearest_anchoring_height(state.height());
        let block_hash = schema.heights().get(height)?.unwrap();
        Ok((height, block_hash))
    }

    pub fn proposal_tx(&self) -> Option<AnchoringTx> {
        self.service_state().proposal_tx.clone()
    }

    pub fn check_lect(&self, state: &NodeState) -> Result<LectKind, StorageError> {
        let anchoring_schema = AnchoringSchema::new(state.view());

        let our_lects = anchoring_schema.lects(state.id());
        if let Some(our_lect) = our_lects.last()? {
            let mut count = 1;
            for id in 0..state.validators().len() as u32 {
                let lects = anchoring_schema.lects(id);
                if Some(&our_lect) == lects.last()?.as_ref() {
                    count += 1;
                }
            }

            if count >= self.majority_count(state)? {
                Ok(LectKind::Some(our_lect))
            } else {
                Ok(LectKind::Different)
            }
        } else {
            Ok(LectKind::None)
        }
    }

    pub fn avaliable_funding_tx(&self, state: &NodeState) -> Result<Option<FundingTx>, RpcError> {
        let (_, genesis) = self.actual_config(state).unwrap();

        let multisig = genesis.multisig();
        if let Some(info) = genesis.funding_tx.is_unspent(&self.client, &multisig)? {
            if info.confirmations >= genesis.utxo_confirmations {
                return Ok(Some(genesis.funding_tx));
            }
        }
        Ok(None)
    }

    // Пытаемся обновить нашу последнюю известную анкорящую транзакцию
    // Помимо этого, если мы обнаруживаем, что она набрала достаточно подтверждений
    // для перехода на новый адрес, то переходим на него
    pub fn update_our_lect(&self, state: &mut NodeState) -> Result<(), RpcError> {
        if state.height() % self.cfg.check_lect_frequency == 0 {
            debug!("Update our lect");
            let (_, genesis) = self.actual_config(state).unwrap();
            let multisig = genesis.multisig();

            let lect = self.client().find_lect(&multisig)?;
            let our_lect = AnchoringSchema::new(state.view()).lects(state.id()).last().unwrap();
            // We needs to update our lect
            if lect != our_lect && lect.is_some() {
                // TODO проверить, что у транзакции есть известный нам prev_hash
                let lect = lect.unwrap();
                debug!("Found new lect={:#?}", lect);

                info!("LECT ====== txid={}", lect.txid().to_hex());
                let lect_msg = TxAnchoringUpdateLatest::new(&state.public_key(),
                                                            state.id(),
                                                            &lect.serialize(),
                                                            &state.secret_key());
                state.add_transaction(AnchoringTransaction::UpdateLatest(lect_msg));
            } else {
                // TODO проверяем ситуацию с пересылкой на новый адрес
            }
        }
        Ok(())
    }

    pub fn try_create_proposal_tx(&self, state: &mut NodeState) -> Result<(), RpcError> {
        match self.check_lect(state).unwrap() {
            LectKind::Different => Ok(()),
            LectKind::None => self.create_first_proposal_tx(state),
            LectKind::Some(tx) => {
                let (_, genesis) = self.actual_config(state).unwrap();
                let anchored_height = tx.payload().0;
                if genesis.nearest_anchoring_height(state.height()) > anchored_height {
                    return self.create_proposal_tx(state, tx);
                }
                Ok(())
            }
        }
    }

    pub fn create_proposal_tx(&self,
                              state: &mut NodeState,
                              lect: AnchoringTx)
                              -> Result<(), RpcError> {
        let (priv_key, genesis) = self.actual_config(state).unwrap();
        let genesis: AnchoringConfig = genesis;

        // Create proposal tx
        let from = genesis.multisig();
        let to = self.output_address(state);
        let (height, hash) = self.actual_payload(state).unwrap();
        let funding_tx = self.avaliable_funding_tx(state)?
            .into_iter()
            .collect::<Vec<_>>();
        let proposal = lect.proposal(&self.client,
                      &from,
                      &to,
                      genesis.fee,
                      &funding_tx,
                      height,
                      hash)?;
        // Sign proposal
        self.sign_proposal_tx(state, proposal, &from, &priv_key)
    }

    // Create first anchoring tx proposal from funding tx in AnchoringNodeConfig
    pub fn create_first_proposal_tx(&self, state: &mut NodeState) -> Result<(), RpcError> {
        debug!("Create first proposal tx");

        let funding_tx = self.avaliable_funding_tx(state)?
            .expect("Funding transaction is not suitable.");
        // Create anchoring proposal
        let (height, hash) = self.actual_payload(state).unwrap();

        let (priv_key, genesis) = self.actual_config(state).unwrap();
        let multisig = genesis.multisig();
        let proposal =
            funding_tx.make_anchoring_tx(&self.client, &multisig, genesis.fee, height, hash)?;

        debug!("initial_proposal={:#?}, txhex={}", proposal, proposal.0.to_hex());

        // Sign proposal
        self.sign_proposal_tx(state, proposal, &multisig, &priv_key)
    }

    pub fn sign_proposal_tx(&self,
                            state: &mut NodeState,
                            proposal: AnchoringTx,
                            multisig: &MultiSig,
                            private_key: &BitcoinPrivateKey)
                            -> Result<(), RpcError> {
        debug!("sign proposal tx");
        let signature = proposal.sign(&self.client, &multisig, 0, &private_key)?;

        debug!("Anchoring propose_tx={:#?}, txhex={}, signature={:?}",
               proposal,
               proposal.0.to_hex(),
               signature.to_hex());

        let sign_msg = TxAnchoringSignature::new(state.public_key(),
                                                 state.id(),
                                                 &proposal.clone().serialize(),
                                                 &signature,
                                                 state.secret_key());
        debug!("Signed txhex={}, txid={}, signature={}",
               proposal.0.to_hex(),
               proposal.txid().to_hex(),
               signature.to_hex());

        self.service_state().proposal_tx = Some(proposal);
        state.add_transaction(AnchoringTransaction::Signature(sign_msg));
        Ok(())
    }

    pub fn try_finalize_proposal_tx(&self,
                                    state: &mut NodeState,
                                    proposal: AnchoringTx)
                                    -> Result<(), RpcError> {
        debug!("try finalize proposal tx");
        let txid = proposal.txid();
        let (_, genesis) = self.actual_config(state).unwrap();

        let proposal_height = proposal.payload().0;
        if genesis.nearest_anchoring_height(state.height()) !=
           genesis.nearest_anchoring_height(proposal_height) {
            warn!("Unable to finalize anchoring tx for height={}",
                  proposal_height);
            self.service_state().proposal_tx = None;
            return Ok(());
        }

        let msgs = AnchoringSchema::new(state.view())
            .signatures(&txid)
            .values()
            .unwrap();

        let majority_count = genesis.majority_count() as usize;
        if msgs.len() >= majority_count {
            let mut signatures = vec![None; genesis.validators.len()];
            for msg in msgs {
                signatures[msg.validator() as usize] = Some(msg.signature().to_vec());
            }

            let signatures = signatures.into_iter()
                .filter_map(|x| x)
                .take(majority_count)
                .collect::<Vec<_>>();

            // FIXME Rewrite me!!!!!!
            let signatures = Some((0, signatures))
                .into_iter()
                .collect::<HashMap<_, _>>();

            let new_lect = proposal.finalize(self.client(), &genesis.multisig(), signatures)?;
            if new_lect.get_info(self.client())?.is_none() {
                self.client.send_transaction(new_lect.0.clone())?;
            }

            info!("ANCHORING ====== anchored_height={}, txid={}, remaining_funds={}",
                  new_lect.payload().0,
                  new_lect.txid().to_hex(),
                  new_lect.funds(new_lect.out(&genesis.multisig())));

            self.service_state().proposal_tx = None;
            let lect_msg = TxAnchoringUpdateLatest::new(state.public_key(),
                                                        state.id(),
                                                        &new_lect.serialize(),
                                                        state.secret_key());
            state.add_transaction(AnchoringTransaction::UpdateLatest(lect_msg));
        }
        Ok(())
    }
}

impl Transaction for AnchoringTransaction {
    fn verify(&self) -> bool {
        self.verify_signature(self.from())
    }

    fn execute(&self, view: &View) -> Result<(), StorageError> {
        let schema = AnchoringSchema::new(view);

        // TODO verify that from validators??
        match *self {
            AnchoringTransaction::Signature(ref sign) => {
                let tx = AnchoringTx::deserialize(sign.tx().to_vec());
                schema.signatures(&tx.txid()).append(sign.clone())
            }
            AnchoringTransaction::UpdateLatest(ref lect) => {
                let tx = AnchoringTx::deserialize(lect.tx().to_vec());
                schema.lects(lect.validator()).append(tx)
            }
        }
    }
}

impl Service for AnchoringService {
    fn service_id(&self) -> u16 {
        ANCHORING_SERVICE
    }

    fn state_hash(&self, _: &View) -> Result<Vec<Hash>, StorageError> {
        Ok(Vec::new())
    }

    fn tx_from_raw(&self, raw: RawTransaction) -> Result<Box<Transaction>, MessageError> {
        AnchoringTransaction::from_raw(raw).map(|tx| Box::new(tx) as Box<Transaction>)
    }

    fn handle_genesis_block(&self, view: &View) -> Result<Value, StorageError> {
        let cfg = self.genesis.clone();
        let redeem_script = cfg.redeem_script();
        self.client
            .importaddress(&redeem_script.to_address(BITCOIN_NETWORK),
                           "multisig",
                           false,
                           false)
            .unwrap();

        AnchoringSchema::new(view).create_genesis_config()?;
        Ok(cfg.to_json())
    }

    fn handle_commit(&self, state: &mut NodeState) -> Result<(), StorageError> {
        debug!("Handle commit, height={}", state.height());
        // First of all we try to update our lect and actual configuration
        self.update_our_lect(state)
            .log_error("Unable to update lect")
            .unwrap();

        // Now if we have anchoring tx proposal we must try to finalize it
        if let Some(proposal) = self.proposal_tx() {
            self.try_finalize_proposal_tx(state, proposal)
                .log_error("Unable to finalize proposal tx")
                .unwrap();
        } else {
            // Or try to create proposal
            self.try_create_proposal_tx(state)
                .log_error("Unable to create proposal tx")
                .unwrap();
        }
        Ok(())
    }
}

trait LogError: Sized {
    fn log_error<S: AsRef<str>>(self, msg: S) -> Self;
}

impl<T, E> LogError for ::std::result::Result<T, E>
    where E: ::std::fmt::Display
{
    fn log_error<S: AsRef<str>>(self, msg: S) -> Self {
        if let Err(ref error) = self {
            error!("{}, an error occured: {}", msg.as_ref(), error);
        }
        self
    }
}
