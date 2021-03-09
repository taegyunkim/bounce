use super::{BounceConfig, CubesatRequest, CubesatResponse};
use bls_signatures_rs::bn256::Bn256;
use bls_signatures_rs::MultiSignature;
use rand::{thread_rng, Rng};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio::time::{interval, interval_at, Instant};

#[derive(Clone, Debug)]
pub enum CommitType {
    Precommit,
    Noncommit,
}

#[derive(Clone, Debug)]
pub struct Commit {
    typ: CommitType,
    // The id of signer
    id: usize,
    // signer's public key
    public_key: Vec<u8>,
    signature: Vec<u8>,
}

#[derive(Clone, Debug)]
pub enum Command {
    Stop,
    // Sign this message sent from ground station
    Sign(Vec<u8>),
    // Aggregate either precommit or noncommit
    Aggregate(Commit),
}

pub enum Phase {
    Stop,
    First,
    Second,
    Third,
}

pub struct Cubesat {
    id: usize,
    slot_id: usize,

    // Configuration for slot
    bounce_config: BounceConfig,
    phase: Phase,

    // Whether this cubesat has signed a precommit or non-commit for current slot
    signed: bool,
    // Whether this cubesat has aggregated signatures of at least supermajority of num_cubesats
    aggregated: bool,

    public_key: Vec<u8>,
    private_key: Vec<u8>,

    // (id, signature) of precommtis or noncommits received for this slot.
    precommits: Vec<Commit>,
    noncommits: Vec<Commit>,

    // sender to send to communications hub
    result_tx: mpsc::Sender<Command>,
    // receiver to receive commands from the communications hub
    request_rx: mpsc::Receiver<Command>,
}

impl Cubesat {
    pub fn new(
        id: usize,
        bounce_config: BounceConfig,
        result_tx: mpsc::Sender<Command>,
        request_rx: mpsc::Receiver<Command>,
    ) -> Self {
        let mut rng = thread_rng();

        // generate public and private key pairs.
        let private_key: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
        let public_key = Bn256.derive_public_key(&private_key).unwrap();

        Cubesat {
            id,
            slot_id: 0,
            bounce_config,
            phase: Phase::Stop,
            signed: false,
            aggregated: false,
            public_key,
            private_key,
            precommits: Vec::new(),
            noncommits: Vec::new(),
            result_tx,
            request_rx,
        }
    }

    pub async fn run(&mut self) {
        let slot_duration = Duration::from_secs(self.bounce_config.slot_duration);
        let mut slot_ticker = interval(slot_duration);
        let start = Instant::now();
        let phase2_start = start + Duration::from_secs(self.bounce_config.phase1_duration);
        let phase3_start = phase2_start + Duration::from_secs(self.bounce_config.phase2_duration);
        let mut phase2_ticker = interval_at(phase2_start, slot_duration);
        let mut phase3_ticker = interval_at(phase3_start, slot_duration);

        loop {
            tokio::select! {
                _ = slot_ticker.tick() => {
                    // Have to sign and send noncommit for (j + 1, i)

                    self.precommits.clear();
                    self.noncommits.clear();
                    self.phase = Phase::First;
                    self.signed = false;
                    self.aggregated = false;
                    println!("slot timer tick");
                    self.slot_id += 1;
                }
                _ = phase2_ticker.tick() => {
                    self.phase = Phase::Second;
                }
                _ = phase3_ticker.tick() => {
                    self.phase = Phase::Third;
                }
                Some(cmd) = self.request_rx.recv() => {
                    match cmd {
                        Command::Stop => {
                            self.phase = Phase::Stop;
                            println!("exiting the loop...");
                            break;
                        }
                        Command::Sign(msg) => {
                            if self.signed {
                                // already signed for this slot.
                                return;
                            }

                            let signature = Bn256.sign(&self.private_key, &msg).unwrap();

                            // TODO: check errors
                            self.result_tx.send(
                                Command::Aggregate(Commit {typ: CommitType::Precommit, id: self.id,
                                    public_key: self.public_key.clone(), signature})
                            ).await.unwrap();
                        }
                        Command::Aggregate(commit) => {
                            match commit.typ {
                                CommitType::Precommit => {
                                    self.precommits.push(commit);

                                    if self.precommits.len() == self.bounce_config.num_cubesats {
                                        let sig_refs :Vec<&[u8]> = self.precommits.iter().map(|p| p.signature.as_slice()).collect();
                                        let aggregate_signature = Bn256.aggregate_signatures(&sig_refs).unwrap();
                                        let public_key_refs: Vec<&[u8]> = self.precommits.iter().map(|p| p.public_key.as_slice()).collect();
                                        let aggregate_public_key = Bn256.aggregate_public_keys(&public_key_refs).unwrap();

                                        let cubesat_response = CubesatResponse {
                                            signature: aggregate_signature,
                                            public_key: aggregate_public_key,
                                        };


                                    }
                                }
                                CommitType::Noncommit => {
                                    self.noncommits.push(commit);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cubesat_stop() {
        let (result_tx, _) = mpsc::channel(1);
        let (request_tx, request_rx) = mpsc::channel(15);

        let mut c = Cubesat::new(
            0,
            BounceConfig {
                num_cubesats: 1,
                slot_duration: 10,
                phase1_duration: 4,
                phase2_duration: 4,
            },
            result_tx,
            request_rx,
        );

        tokio::spawn(async move {
            c.run().await;
        });

        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        request_tx
            .send(Command::Stop)
            .await
            .expect("Failed to send stop command");
    }

    #[tokio::test]
    async fn cubesat_sign() {
        let (result_tx, mut result_rx) = mpsc::channel(1);
        let (request_tx, request_rx) = mpsc::channel(15);

        let mut c = Cubesat::new(
            0,
            BounceConfig {
                num_cubesats: 1,

                slot_duration: 10,
                phase1_duration: 4,
                phase2_duration: 4,
            },
            result_tx,
            request_rx,
        );

        tokio::spawn(async move {
            c.run().await;
        });

        tokio::spawn(async move {
            request_tx
                .send(Command::Sign("hello".as_bytes().to_vec()))
                .await
                .expect("failed to send sign command");
        });

        let result_opt = result_rx.recv().await;
        assert!(result_opt.is_some());
        let cmd = result_opt.unwrap();
        assert!(matches!(cmd, Command::Aggregate { .. }));
    }
}
