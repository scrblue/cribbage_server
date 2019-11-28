extern crate cribbage;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net;
use std::sync::mpsc;
use std::time::{Duration, Instant};

// Handles input and output to each client
pub fn handle_client(
    // The TCP stream the handler takes for the client given when spawning the thread
    mut client_stream: net::TcpStream,
    // The transmitter used to send messages to the game thread; shared with the other clients
    game_handler_transmitter: mpsc::Sender<super::messages::ClientToGame>,
    game_handler_receiver: mpsc::Receiver<super::messages::GameToClient>,
) {
    let mut is_disconncted = false;

    game_handler_transmitter
        .send(super::messages::ClientToGame::Greeting)
        .unwrap();

    // While the connection is accepted
    while !is_disconncted {
        // Forward message from receiver to the client then wait for client response
        let mut packet_from_client = [0 as u8; 256];
        match game_handler_receiver.recv() {
            // When all the maximum number of players has been connected and the connection is
            // denied
            Ok(super::messages::GameToClient::DeniedTableFull) => {
                client_stream
                    .write(
                        &bincode::serialize(&super::messages::GameToClient::DeniedTableFull)
                            .unwrap(),
                    )
                    .unwrap();
                client_stream.flush().unwrap();
                game_handler_transmitter
                    .send(super::messages::ClientToGame::TransmissionReceived)
                    .unwrap();
            }

            // Accepts the player's name
            Ok(super::messages::GameToClient::WaitName) => {
                let mut valid_name = false;
                while !valid_name {
                    client_stream
                        .write(
                            &bincode::serialize(&super::messages::GameToClient::WaitName).unwrap(),
                        )
                        .unwrap();
                    client_stream.flush().unwrap();
                    game_handler_transmitter
                        .send(super::messages::ClientToGame::TransmissionReceived)
                        .unwrap();
                    client_stream.read(&mut packet_from_client).unwrap();
                    let client_to_game: super::messages::ClientToGame =
                        bincode::deserialize(&packet_from_client).unwrap();
                    match client_to_game {
                        super::messages::ClientToGame::Name(name) => {
                            valid_name = true;
                            game_handler_transmitter
                                .send(super::messages::ClientToGame::Name(name))
                                .unwrap();
                            packet_from_client = [0 as u8; 256];
                        }
                        _ => {
                            packet_from_client = [0 as u8; 256];
                        }
                    };
                }
            }

            // Forwards the notification
            Ok(super::messages::GameToClient::PlayerJoinNotification { name, number, of }) => {
                client_stream
                    .write(
                        &bincode::serialize(
                            &super::messages::GameToClient::PlayerJoinNotification {
                                name: name,
                                number: number,
                                of: of,
                            },
                        )
                        .unwrap(),
                    )
                    .unwrap();

                client_stream.flush().unwrap();
                game_handler_transmitter
                    .send(super::messages::ClientToGame::TransmissionReceived)
                    .unwrap();
            }

            // Forwards the request for the cut and waits for a response from the client
            Ok(super::messages::GameToClient::WaitInitialCut) => {
                let mut has_sent_confirmation = false;
                while !has_sent_confirmation {
                    client_stream
                        .write(
                            &bincode::serialize(&super::messages::GameToClient::WaitInitialCut)
                                .unwrap(),
                        )
                        .unwrap();

                    client_stream.flush().unwrap();
                    game_handler_transmitter
                        .send(super::messages::ClientToGame::TransmissionReceived)
                        .unwrap();

                    client_stream.read(&mut packet_from_client).unwrap();

                    let client_to_game: super::messages::ClientToGame =
                        bincode::deserialize(&packet_from_client).unwrap();

                    match client_to_game {
                        super::messages::ClientToGame::Confirmation => {
                            game_handler_transmitter
                                .send(super::messages::ClientToGame::Confirmation)
                                .unwrap();
                            has_sent_confirmation = true;
                            packet_from_client = [0 as u8; 256];
                        }
                        _ => {
                            packet_from_client = [0 as u8; 256];
                        }
                    }
                }
            }

            // Simple forwarding
            Ok(super::messages::GameToClient::InitialCutResult { name, card }) => {
                client_stream
                    .write(
                        &bincode::serialize(&super::messages::GameToClient::InitialCutResult {
                            name: name,
                            card: card,
                        })
                        .unwrap(),
                    )
                    .unwrap();

                client_stream.flush().unwrap();
                game_handler_transmitter
                    .send(super::messages::ClientToGame::TransmissionReceived)
                    .unwrap();
            }

            Ok(super::messages::GameToClient::InitialCutSuccess(string)) => {
                client_stream
                    .write(
                        &bincode::serialize(&super::messages::GameToClient::InitialCutSuccess(
                            string,
                        ))
                        .unwrap(),
                    )
                    .unwrap();

                client_stream.flush().unwrap();
                game_handler_transmitter
                    .send(super::messages::ClientToGame::TransmissionReceived)
                    .unwrap();
            }

            Ok(super::messages::GameToClient::InitialCutFailure) => {
                client_stream
                    .write(
                        &bincode::serialize(&super::messages::GameToClient::InitialCutFailure)
                            .unwrap(),
                    )
                    .unwrap();

                client_stream.flush().unwrap();
                game_handler_transmitter
                    .send(super::messages::ClientToGame::TransmissionReceived)
                    .unwrap();
            }

            Ok(super::messages::GameToClient::WaitDeal) => {
                let mut has_sent_confirmation = false;
                while !has_sent_confirmation {
                    client_stream
                        .write(
                            &bincode::serialize(&super::messages::GameToClient::WaitDeal).unwrap(),
                        )
                        .unwrap();

                    client_stream.flush().unwrap();
                    game_handler_transmitter
                        .send(super::messages::ClientToGame::TransmissionReceived)
                        .unwrap();

                    client_stream.read(&mut packet_from_client).unwrap();

                    let client_to_game: super::messages::ClientToGame =
                        bincode::deserialize(&packet_from_client).unwrap();

                    match client_to_game {
                        super::messages::ClientToGame::Confirmation => {
                            game_handler_transmitter
                                .send(super::messages::ClientToGame::Confirmation)
                                .unwrap();
                            has_sent_confirmation = true;
                            packet_from_client = [0 as u8; 256];
                        }
                        _ => {
                            packet_from_client = [0 as u8; 256];
                        }
                    }
                }
            }

            Ok(super::messages::GameToClient::Dealing) => {
                client_stream
                    .write(&bincode::serialize(&super::messages::GameToClient::Dealing).unwrap())
                    .unwrap();

                client_stream.flush().unwrap();
                game_handler_transmitter
                    .send(super::messages::ClientToGame::TransmissionReceived)
                    .unwrap();
            }

            Ok(super::messages::GameToClient::DealtHand(hand)) => {
                client_stream
                    .write(
                        &bincode::serialize(&super::messages::GameToClient::DealtHand(hand))
                            .unwrap(),
                    )
                    .unwrap();

                client_stream.flush().unwrap();
                game_handler_transmitter
                    .send(super::messages::ClientToGame::TransmissionReceived)
                    .unwrap();
            }

            Ok(super::messages::GameToClient::Disconnect) => {
                is_disconncted = true;
                client_stream
                    .write(&bincode::serialize(&super::messages::GameToClient::Disconnect).unwrap())
                    .unwrap();

                client_stream.flush().unwrap();
                game_handler_transmitter
                    .send(super::messages::ClientToGame::TransmissionReceived)
                    .unwrap();
            }

            _ => {
                println!("Unexpected message from game handler");
                client_stream
                    .write(
                        &bincode::serialize(&super::messages::GameToClient::Error(String::from(
                            "Unexpected message from game handler",
                        )))
                        .unwrap(),
                    )
                    .unwrap();

                client_stream.flush().unwrap();
                game_handler_transmitter
                    .send(super::messages::ClientToGame::TransmissionReceived)
                    .unwrap();
                is_disconncted = true;
            }
        }
    }
}
