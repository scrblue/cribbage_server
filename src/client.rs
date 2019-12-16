extern crate cribbage;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net;
use std::sync::mpsc;

// Simply sends the message to the client and guarantees its arrival
fn simple_notification(
    client_stream: &mut net::TcpStream,
    game_handler_transmitter: &mpsc::Sender<super::messages::ClientToGame>,
    message: super::messages::GameToClient,
) {
    client_stream
        .write(&bincode::serialize(&message).unwrap())
        .unwrap();
    client_stream.flush().unwrap();
    game_handler_transmitter
        .send(super::messages::ClientToGame::TransmissionReceived)
        .unwrap();
}

// Polls for a Confirmation message from the client and forwards it to the game handler when it is
// received
fn confirmation_request(
    client_stream: &mut net::TcpStream,
    game_handler_transmitter: &mpsc::Sender<super::messages::ClientToGame>,
    message: super::messages::GameToClient,
) {
    let mut packet_from_client = [0 as u8; 256];
    let mut has_sent_confirmation = false;
    while !has_sent_confirmation {
        simple_notification(client_stream, game_handler_transmitter, message.clone());

        client_stream.read(&mut packet_from_client).unwrap();

        let client_to_game: super::messages::ClientToGame =
            bincode::deserialize(&packet_from_client).unwrap();

        match client_to_game {
            super::messages::ClientToGame::Confirmation => {
                game_handler_transmitter
                    .send(super::messages::ClientToGame::Confirmation)
                    .unwrap();
                has_sent_confirmation = true;
            }
            _ => {
                packet_from_client = [0 as u8; 256];
            }
        }
    }
}

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
        match game_handler_receiver.recv() {
            // When all the maximum number of players has been connected and the connection is
            // denied
            Ok(super::messages::GameToClient::DeniedTableFull) => {
                simple_notification(
                    &mut client_stream,
                    &game_handler_transmitter,
                    super::messages::GameToClient::DeniedTableFull,
                );
            }

            // Accepts the player's name
            // TODO Confirm name is not already in use
            Ok(super::messages::GameToClient::WaitName) => {
                let mut packet_from_client = [0 as u8; 256];
                let mut valid_name = false;
                while !valid_name {
                    simple_notification(
                        &mut client_stream,
                        &game_handler_transmitter,
                        super::messages::GameToClient::WaitName,
                    );
                    client_stream.read(&mut packet_from_client).unwrap();
                    let client_to_game: super::messages::ClientToGame =
                        bincode::deserialize(&packet_from_client).unwrap();
                    match client_to_game {
                        super::messages::ClientToGame::Name(name) => {
                            valid_name = true;
                            game_handler_transmitter
                                .send(super::messages::ClientToGame::Name(name))
                                .unwrap();
                        }
                        _ => {
                            packet_from_client = [0 as u8; 256];
                        }
                    };
                }
            }

            Ok(super::messages::GameToClient::PlayerJoinNotification { name, number, of }) => {
                simple_notification(
                    &mut client_stream,
                    &game_handler_transmitter,
                    super::messages::GameToClient::PlayerJoinNotification { name, number, of },
                )
            }

            Ok(super::messages::GameToClient::WaitInitialCut) => confirmation_request(
                &mut client_stream,
                &game_handler_transmitter,
                super::messages::GameToClient::WaitInitialCut,
            ),

            Ok(super::messages::GameToClient::InitialCutResult { name, card }) => {
                simple_notification(
                    &mut client_stream,
                    &game_handler_transmitter,
                    super::messages::GameToClient::InitialCutResult { name, card },
                );
            }

            Ok(super::messages::GameToClient::InitialCutSuccess(name)) => {
                simple_notification(
                    &mut client_stream,
                    &game_handler_transmitter,
                    super::messages::GameToClient::InitialCutSuccess(name),
                );
            }

            Ok(super::messages::GameToClient::InitialCutFailure) => {
                simple_notification(
                    &mut client_stream,
                    &game_handler_transmitter,
                    super::messages::GameToClient::InitialCutFailure,
                );
            }

            Ok(super::messages::GameToClient::WaitDeal) => {
                confirmation_request(
                    &mut client_stream,
                    &game_handler_transmitter,
                    super::messages::GameToClient::WaitDeal,
                );
            }

            Ok(super::messages::GameToClient::Dealing) => {
                simple_notification(
                    &mut client_stream,
                    &game_handler_transmitter,
                    super::messages::GameToClient::Dealing,
                );
            }

            Ok(super::messages::GameToClient::DealtHand(hand)) => {
                simple_notification(
                    &mut client_stream,
                    &game_handler_transmitter,
                    super::messages::GameToClient::DealtHand(hand),
                );
            }

            Ok(super::messages::GameToClient::WaitDiscardOne) => {
                let mut packet_from_client = [0 as u8; 256];

                simple_notification(
                    &mut client_stream,
                    &game_handler_transmitter,
                    super::messages::GameToClient::WaitDiscardOne,
                );

                // Check for input from the client and forward DiscardPlaced messages
                client_stream.set_nonblocking(true).unwrap();

                let mut received_discard_message = false;
                while !received_discard_message {
                    match client_stream.read(&mut packet_from_client) {
                        Ok(_) => {
                            let client_to_game: super::messages::ClientToGame =
                                bincode::deserialize(&packet_from_client).unwrap();
                            match client_to_game {
                                super::messages::ClientToGame::DiscardOne { index } => {
                                    game_handler_transmitter
                                        .send(super::messages::ClientToGame::DiscardOne { index })
                                        .unwrap();
                                    received_discard_message = true;
                                    packet_from_client = [0 as u8; 256];
                                }
                                _ => {
                                    packet_from_client = [0 as u8; 256];
                                }
                            }
                        }
                        _ => {}
                    };
                    match game_handler_receiver.try_recv() {
                        Ok(super::messages::GameToClient::DiscardPlacedOne(name)) => simple_notification(
                            &mut client_stream,
                            &game_handler_transmitter,
                            super::messages::GameToClient::DiscardPlacedOne(name),
                        ),
                        Ok(_) => println!("Invalid message to client when trying to receive a DiscardPlacedOne message"),
                        _ => {},
                    };
                }

                client_stream.set_nonblocking(false).unwrap();
            }
            Ok(super::messages::GameToClient::WaitDiscardTwo) => {
                let mut packet_from_client = [0 as u8; 256];

                simple_notification(
                    &mut client_stream,
                    &game_handler_transmitter,
                    super::messages::GameToClient::WaitDiscardTwo,
                );

                // Check for input from the client and forward DiscardPlaced messages
                client_stream.set_nonblocking(true).unwrap();

                let mut received_discard_message = false;
                while !received_discard_message {
                    match client_stream.read(&mut packet_from_client) {
                        Ok(_) => {
                            let client_to_game: super::messages::ClientToGame =
                                bincode::deserialize(&packet_from_client).unwrap();
                            match client_to_game {
                                super::messages::ClientToGame::DiscardTwo {
                                    index_one,
                                    index_two,
                                } => {
                                    game_handler_transmitter
                                        .send(super::messages::ClientToGame::DiscardTwo {
                                            index_one,
                                            index_two,
                                        })
                                        .unwrap();
                                    received_discard_message = true;
                                    packet_from_client = [0 as u8; 256];
                                }
                                _ => {
                                    packet_from_client = [0 as u8; 256];
                                }
                            }
                        }
                        _ => {}
                    };
                    match game_handler_receiver.try_recv() {
                        Ok(super::messages::GameToClient::DiscardPlacedTwo(name)) => simple_notification(
                            &mut client_stream,
                            &game_handler_transmitter,
                            super::messages::GameToClient::DiscardPlacedTwo(name),
                        ),
                        Ok(_) => println!("Invalid message to client when trying to receive a DiscardPlacedTwo message"),
                        _ => {},
                    };
                }

                client_stream.set_nonblocking(false).unwrap();
            }

            Ok(super::messages::GameToClient::DiscardPlacedOne(name)) => simple_notification(
                &mut client_stream,
                &game_handler_transmitter,
                super::messages::GameToClient::DiscardPlacedOne(name),
            ),
            Ok(super::messages::GameToClient::DiscardPlacedTwo(name)) => simple_notification(
                &mut client_stream,
                &game_handler_transmitter,
                super::messages::GameToClient::DiscardPlacedTwo(name),
            ),

            Ok(super::messages::GameToClient::AllDiscards) => {
                simple_notification(
                    &mut client_stream,
                    &game_handler_transmitter,
                    super::messages::GameToClient::AllDiscards,
                );
            }

            Ok(super::messages::GameToClient::CutStarter(name, card)) => {
                println!("Sending CutStarter");
                simple_notification(
                    &mut client_stream,
                    &game_handler_transmitter,
                    super::messages::GameToClient::CutStarter(name, card),
                );
                println!("Sent CutStarter");
            }

            Ok(super::messages::GameToClient::WaitCutStarter) => {
                confirmation_request(
                    &mut client_stream,
                    &game_handler_transmitter,
                    super::messages::GameToClient::WaitCutStarter,
                );
            }

            Ok(super::messages::GameToClient::Disconnect) => {
                is_disconncted = true;
                simple_notification(
                    &mut client_stream,
                    &game_handler_transmitter,
                    super::messages::GameToClient::Disconnect,
                );
            }

            _ => {
                println!("Unexpected message from game handler");
                simple_notification(
                    &mut client_stream,
                    &game_handler_transmitter,
                    super::messages::GameToClient::Error(String::from(
                        "Unexpected message from game handler",
                    )),
                );

                is_disconncted = true;
            }
        }
    }
}
