extern crate cribbage;
use std::sync::mpsc;

// A structure tying a player index in the game to a transmitter and receiver for a client handling
// thread
struct GameClientInterface {
    index: u8,
    transmitter: mpsc::Sender<super::messages::GameToClient>,
    receiver: mpsc::Receiver<super::messages::ClientToGame>,
}

// Handles the game object
pub fn handle_game(
    mut game_object: cribbage::Game,
    num_players: u8,
    man_scoring: bool,
    underpegging: bool,
    muggins: bool,
    overpegging: bool,
    // Facilitates communication between the main thread and the game thread
    main_receiver: mpsc::Receiver<super::messages::MainToGame>,
    main_transmitter: mpsc::Sender<super::messages::GameToMain>,
) {
    // A vector containing the game player index that matches the client thread that the
    // transmitter and receiver comunicate with
    let mut client_interfaces: Vec<GameClientInterface> = Vec::new();
    // A list of player names to be used in constructing the the game object
    // Adds empty names to start as the names may be received out of order so they must be indexed
    // directly
    let mut names: Vec<String> = Vec::new();
    for _i in 0..num_players {
        names.push(String::new());
    }
    // A variable holding the output of the game loop
    let mut output: Result<&str, &str> = Ok("Game thread running");

    while game_object.state != cribbage::GameState::End && output.is_ok() {
        // Request names and game setup
        output = match game_object.state {
            cribbage::GameState::GameStart => {
                let mut num_connected = 0;
                let mut num_named = 0;
                // Runs a loop polling for new interfaces sent from the main thread and deals with
                // the first few messages to be sent from/to the client
                while num_named < num_players {
                    let new_gci: Option<GameClientInterface> = match main_receiver.try_recv() {
                        Ok(super::messages::MainToGame::NewClient {
                            transmitter,
                            receiver,
                        }) => Some(GameClientInterface {
                            index: num_connected,
                            transmitter: transmitter,
                            receiver: receiver,
                        }),
                        _ => None,
                    };

                    // Add the GameClientInterface to the vector if one was sent
                    if new_gci.is_some() {
                        client_interfaces.push(new_gci.unwrap());
                        num_connected += 1;
                    }

                    // Retain instead of iterating over the vector because the request can be
                    // denied
                    client_interfaces.retain(|gci| {
                        // If a message has been sent by any of the clients
                        match gci.receiver.try_recv() {
                            Ok(super::messages::ClientToGame::Greeting) => {
                                // Client has initiated communications
                                if num_connected <= num_players {
                                    // So send a request asking for their name if there is spot
                                    // open
                                    gci.transmitter
                                        .send(super::messages::GameToClient::WaitName);
                                    true
                                } else {
                                    // Or send a denial and delete the connection if there are too many players
                                    gci.transmitter
                                        .send(super::messages::GameToClient::DeniedTableFull);
                                    false
                                }
                            }
                            Ok(super::messages::ClientToGame::Name(name)) => {
                                // When given a name add the player to the count and add their name
                                // to the corresponding location in the name vector
                                names[gci.index as usize] = name;
                                num_named += 1;
                                // And notify the clients of the person joining
                                /*for client_interface in &client_interfaces {
                                    client_interface.transmitter.send(
                                        super::messages::GameToClient::PlayerJoinNotification {
                                            name: name.clone(),
                                            number: num_named,
                                            of: num_players,
                                        },
                                    );
                                }*/

                                true
                            }
                            _ => true,
                        }
                    });
                }

                // Proceed the game model
                match game_object.process_event(cribbage::GameEvent::GameSetup {
                    input_manual: man_scoring,
                    input_underscoring: underpegging,
                    input_muggins: muggins,
                    input_overscoring: overpegging,
                    input_player_names: names,
                }) {
                    Ok("Received valid GameSetup event") => {
                        Ok("Connected to proper number of players and setup game")
                    }
                    Err(e) => Err(e),
                    _ => Err("Unexpected result processing GameSetup"),
                }
            }
            cribbage::GameState::CutInitial => {
                let result = game_object.process_event(cribbage::GameEvent::Confirmation);
                // Ask each player for confirmation and display their cut
                for index in 0..num_players {
                    // Send a request for confirmation to each player one by one until a
                    // confirmation message is received and notify each client of the result of the
                    // that player's cut upon receiving the confirmation
                    let confirmation_sent = false;
                    while !confirmation_sent {
                        client_interfaces[index as usize]
                            .transmitter
                            .send(super::messages::GameToClient::WaitInitialCut);
                        if client_interfaces[index as usize].receiver.recv()
                            == Ok(super::messages::ClientToGame::Confirmation)
                        {
                            confirmation_sent = true;
                        }
                    }
                    for index2 in 0..num_players {
                        client_interfaces[index2 as usize].transmitter.send(
                            super::messages::GameToClient::InitialCutResult {
                                name: game_object.players[index as usize].username.clone(),
                                card: game_object.players[index as usize].hand[0],
                            },
                        );
                    }
                }
                // Announce result and proceed accordingly
                match result {
                    Ok("Cut resulted in tie; redoing") => {
                        for client_interface in client_interfaces {
                            client_interface
                                .transmitter
                                .send(super::messages::GameToClient::InitialCutFailure);
                        }
                        Ok("Cut resulted in tie")
                    }
                    Ok("First dealer chosen with cut") => {
                        for client_interface in client_interfaces {
                            client_interface.transmitter.send(
                                super::messages::GameToClient::InitialCutSuccess(
                                    game_object.players[game_object.index_dealer as usize]
                                        .username
                                        .clone(),
                                ),
                            );
                        }
                        Ok("Cut successful, dealer chosen")
                    }
                    _ => Err("Unexpected result of cut"),
                }
            }
            _ => Err("Unexpected game state in game model"),
        }
    }

    match output {
        Ok(_) => println!("Quit game after end state"),
        Err(e) => println!(
            "Quit game after error result to game loop in handle_game; {}",
            e
        ),
    }

    for client_interface in client_interfaces {
        client_interface
            .transmitter
            .send(super::messages::GameToClient::Disconnect);
    }

    main_transmitter
        .send(super::messages::GameToMain::EndServer)
        .unwrap();
}
