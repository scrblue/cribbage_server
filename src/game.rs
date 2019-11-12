extern crate cribbage;
use std::sync::mpsc;

#[derive(PartialEq)]
enum GciState {
    Connecting,
    DeniedPlayerCount,
    AskedName,
    WaitingNames,
    WaitingConfirmationForInitialCut,
    WaitingForInitialCuts,
    End,
}

// A structure tying a player index in the game to a transmitter and receiver for a client handling
// thread
struct GameClientInterface {
    // The index
    index: Option<u8>,
    transmitter: mpsc::Sender<super::messages::GameToClient>,
    receiver: mpsc::Receiver<super::messages::ClientToGame>,
    state: GciState,
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

    // A record of how many clients connected there are total and how many named players there are
    let mut num_connections: u8 = 0;
    let mut named_players: u8 = 0;

    // A variable holding the output of the game loop
    let mut output: Result<&str, &str> = Ok("Game thread running");

    while output.is_ok() && output != Ok("Server ending") {
        output = match game_object.state {
            cribbage::GameState::GameStart => {
                // A list of player names to be used in constructing the the game object
                // Adds empty names to start as the names may be received out of order so they must
                // be indexed  directly
                let mut names: Vec<String> = Vec::new();
                for _i in 0..num_players {
                    names.push(String::new());
                }

                // Run a loop dealing with new clients and receiving names for as long as there are
                // spots open
                while named_players < num_players {
                    // Polls the main_receiver for the transmitter and receivers of a new client
                    let new_gci: Option<GameClientInterface> = match main_receiver.try_recv() {
                        Ok(super::messages::MainToGame::NewClient {
                            transmitter,
                            receiver,
                        }) => Some(GameClientInterface {
                            index: None,
                            transmitter: transmitter,
                            receiver: receiver,
                            state: GciState::Connecting,
                        }),
                        // Returns none if there is no message for the main_receiver
                        _ => None,
                    };
                    // If there is a new GameClientInterface
                    if new_gci.is_some() {
                        num_connections += 1;
                        client_interfaces.push(new_gci.unwrap());
                    }

                    // After the new client or lack thereof is dealt with, poll for relevant
                    // messages from the client threads
                    for index in 0..num_connections {
                        match client_interfaces[index as usize].receiver.try_recv() {
                            Ok(super::messages::ClientToGame::Greeting) => {
                                if num_connections > num_players {
                                    client_interfaces[index as usize]
                                        .transmitter
                                        .send(super::messages::GameToClient::DeniedTableFull)
                                        .unwrap();
                                    client_interfaces[index as usize].state =
                                        GciState::DeniedPlayerCount;
                                } else {
                                    // If there is a spot for the player, request their name
                                    client_interfaces[index as usize]
                                        .transmitter
                                        .send(super::messages::GameToClient::WaitName)
                                        .unwrap();
                                    client_interfaces[index as usize].state = GciState::AskedName;
                                }
                            }
                            // TODO Check if username is already in use
                            Ok(super::messages::ClientToGame::Name(name)) => {
                                names[named_players as usize] = name;
                                client_interfaces[index as usize].index = Some(named_players);
                                named_players += 1;

                                for second_index in 0..num_connections {
                                    client_interfaces[second_index as usize]
                                        .transmitter
                                        .send(
                                            super::messages::GameToClient::PlayerJoinNotification {
                                                name: names[named_players as usize - 1].clone(),
                                                number: named_players,
                                                of: num_players,
                                            },
                                        )
                                        .unwrap();
                                }
                            }
                            // No message on receiver
                            _ => {}
                        }
                    }

                    client_interfaces.retain(|client| {
                        if client.state == GciState::DeniedPlayerCount {
                            num_connections -= 1;
                            false
                        } else {
                            true
                        }
                    });
                }
                match game_object.process_event(cribbage::GameEvent::GameSetup {
                    input_manual: man_scoring,
                    input_underscoring: underpegging,
                    input_muggins: muggins,
                    input_overscoring: overpegging,
                    input_player_names: names,
                }) {
                    Ok("Received valid GameSetup event") => Ok("All players have joined the game"),
                    Err(e) => Err(e),
                    _ => Err("Unexpected process_event result for GameSetup"),
                }
            }

            cribbage::GameState::CutInitial => Err("CutInitial handling not finished"),

            _ => Err("Unexpected state in game_object"),
        };

        println!("{:?}", output);
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
