extern crate cribbage;
use std::sync::mpsc;
use std::{thread, time};

// TODO Handle all the unwraps and do proper error handling and all

#[derive(PartialEq, Debug)]
enum GciState {
    // The state given to a client that has just connected and who has yet to send the greeting
    Connecting,

    // Any clients that are denied because the table is full
    Watching,

    // Any players who have joined the table and who have been asked for a name
    WaitingName,

    // The player who has been requested confirmation for the cut
    WaitingForInitialCut,

    // The dealer who's confirmation deals the hands
    WaitingForDeal,

    // The players who need to select one or two cards to discard
    WaitingForDiscards,

    // When the client has disconnected and the GCI should be cleaned up
    Disconnected,

    // That then client is waiting for a message from the server to continue
    WaitingForServer,
}

// A structure tying a player index in the game to a transmitter and receiver for a client handling
// thread
struct GameClientInterface {
    // The player index in the Game players vector that corresponds to this client
    index: Option<u8>,

    // The transmitter and receiver to the thread handling the client
    transmitter: mpsc::Sender<super::messages::GameToClient>,
    receiver: mpsc::Receiver<super::messages::ClientToGame>,

    // The state of the client, what input is required or that the client is waiting for input from
    // the server
    state: GciState,
}

// A structure used to forward ClientToGame messages from the receiver in the GameClientInterface
// from the loop polling the receiver to the logic of the game such that a mutable reference to the
// client_interfaces is not already used
struct ClientMessage {
    index: u8,
    message: super::messages::ClientToGame,
}

// A struct used for keeping track of the inputs required from each player when players are asked
// for a certain input in the order by player index. For when input is required in order by index
// it tracks the active index, the last active index, the index to stop at, and the input required.
// For example, the active index starting at zero, the index to stop at being num_players - 1, and
// the input required being Confirmation means that every player must provide confirmation in the
// order that they joined the game such as during the initial cut. If the active index starts at
// (index_dealer + 1) % num_players, has no stop point, and requires PlayEvents then it will ask
// for PlayEvents from each player in a circle on loop until the end of the play phase.
struct OrderedInputTracker {
    index_active: u8,
    index_last: Option<u8>,
    index_stop: Option<u8>,
}

// An enum used to keep track of the player inputs that need to be held for the next process_event
// call to the GameObject
enum InputStore {
    Names(Vec<String>),
    Discards(Vec<DiscardSelection>),
}

enum DiscardSelection {
    OneDiscard {
        player_index: u8,
        card_index: u8,
    },
    TwoDiscard {
        player_index: u8,
        card_index_one: u8,
        card_index_two: u8,
    },
}

// Sends a message to a given client interface and assures that the message has been received
// TODO Get it to return an error if the message is not TransmissionReceived instead of using an
// assert
fn send_message(message: super::messages::GameToClient, gci: &mut GameClientInterface) {
    gci.transmitter.send(message).unwrap();
    assert!(gci.receiver.recv() == Ok(super::messages::ClientToGame::TransmissionReceived));
}

// Simply returns whether or not all players in a vector of GameClientInterfaces are waiting for a
// message from the server
fn are_all_players_waiting(gcis: &Vec<GameClientInterface>) -> bool {
    // Whether or not any player isn't waiting
    let mut not_waiting = false;
    for gci in gcis {
        // Only check the GameClientInterface state if the client is a player
        if gci.index.is_some() {
            if gci.state != GciState::WaitingForServer {
                not_waiting = true;
            }
        }
    }

    !not_waiting
}

// Send all hands to each player
// TODO Return error if send fails when error handling is dealt with
fn send_hands(game_object: &cribbage::Game, clients: &mut Vec<GameClientInterface>) {
    for mut client in clients {
        // Only report hands to players
        if client.index.is_some() {
            send_message(
                super::messages::GameToClient::DealtHand(
                    game_object.players[client.index.unwrap() as usize]
                        .hand
                        .clone(),
                ),
                &mut client,
            );
        }
    }
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

    // A vector containing a list of ClietMessages received from any client such that a mutable
    // reference to client_interfaces is only used for the duration of the loop adding client
    // messages
    let mut client_messages: Vec<ClientMessage> = Vec::new();

    // A variable tracking the player indexs that must send input eg. the confirmation call
    let mut input_tracker: Option<OrderedInputTracker> = None;

    // A variable tracking the number of clients that are also players; less than or equal to the
    // number of players
    let mut num_connected_players: u8 = 0;

    // A vector of strings holding player names to be fed to the game_object; loop is to create an
    // empty string for each name so they can be filled out of order
    let mut input_store: InputStore = InputStore::Names(Vec::new());
    for _ in 0..num_players {
        if let InputStore::Names(names) = &mut input_store {
            names.push(String::new());
        }
    }

    // A variable holding the output of the game loop
    let mut output: Result<&str, &str> = Ok("Game thread running");

    // While the output of the game model is valid
    'game_loop: while output.is_ok() && output != Ok("Server ending") {
        // If there is a new client handler thread, create the GameClientInterface
        match main_receiver.try_recv() {
            Ok(super::messages::MainToGame::NewClient {
                transmitter,
                receiver,
            }) => {
                println!("New client_interface");
                client_interfaces.push(GameClientInterface {
                    index: None,
                    state: GciState::Connecting,
                    transmitter: transmitter,
                    receiver: receiver,
                });
            }
            _ => {}
        };

        // For every client (players and spectators)
        for client_interface in &mut client_interfaces {
            // Handle client messages
            match client_interface.receiver.try_recv() {
                // If the client sends a Greeting messages, respond with WaitName or
                // DeniedTableFull depending on the number of player spots left in the game and the
                // game state
                Ok(super::messages::ClientToGame::Greeting) => {
                    println!("Received Greeting");
                    if client_interface.state == GciState::Connecting {
                        if game_object.state == cribbage::GameState::GameStart
                            && num_connected_players < num_players
                        {
                            println!("New player");
                            client_interface.index = Some(num_connected_players);
                            client_interface.state = GciState::WaitingName;
                            send_message(super::messages::GameToClient::WaitName, client_interface);
                            num_connected_players += 1;
                        } else {
                            println!("New watcher");
                            client_interface.state = GciState::Watching;
                            send_message(
                                super::messages::GameToClient::DeniedTableFull,
                                client_interface,
                            );
                        }
                    }
                }

                // Simple forwards to the client_messages vector; ignores input from any client
                // that isn't a player and sends a DeniedTableFull message
                Ok(message) => {
                    if client_interface.index.is_some() {
                        client_messages.push(ClientMessage {
                            index: client_interface.index.unwrap(),
                            message: message,
                        })
                    } else {
                        send_message(
                            super::messages::GameToClient::DeniedTableFull,
                            client_interface,
                        );
                    }
                }

                _ => {}
            }
        }

        // Deal with clients depending on the state of the game and the input received and set the
        // output variable to the sclient_interfaces[input.index as usize].state == GciState::WaitingNametatus message this processing dictates
        output = match game_object.state {
            // If the GameState is GameStart, accept player name messages until the number of
            // connected players equals the number of players in the game and all players are
            // waiting. When both conditions are true, process the GameSetup event
            cribbage::GameState::GameStart => {
                if num_connected_players != num_players
                    || !are_all_players_waiting(&client_interfaces)
                {
                    for input in &client_messages {
                        // If the message reveived is a Name message and the input is from a client
                        // who's state is WaitingName process the name; if the state is not
                        // WaitingName or the message is not a Name, send an error or another
                        // request for the client to send their name
                        if let super::messages::ClientToGame::Name(name) = &input.message {
                            if client_interfaces[input.index as usize].state
                                == GciState::WaitingName
                            {
                                // Set the correct element of names to the received name, change
                                // the client's state, and announce the player joining to all
                                // clients
                                if let InputStore::Names(names) = &mut input_store {
                                    names[input.index as usize] = name.to_string();
                                } else {
                                    output = Err("InputStore not Names");
                                    break 'game_loop;
                                }
                                client_interfaces[input.index as usize].state =
                                    GciState::WaitingForServer;
                                for client_interface in &mut client_interfaces {
                                    send_message(
                                        super::messages::GameToClient::PlayerJoinNotification {
                                            name: name.to_string(),
                                            number: input.index + 1,
                                            of: num_players,
                                        },
                                        client_interface,
                                    )
                                }
                            } else {
                                send_message(
                                    super::messages::GameToClient::Error(
                                        "Input is not required".to_string(),
                                    ),
                                    &mut client_interfaces[input.index as usize],
                                )
                            }
                        }
                        // If the message received is not a Name message and one is required, send
                        // the WaitName message; if it is not required send an error
                        else if client_interfaces[input.index as usize].state
                            == GciState::WaitingName
                        {
                            send_message(
                                super::messages::GameToClient::WaitName,
                                &mut client_interfaces[input.index as usize],
                            )
                        } else {
                            send_message(
                                super::messages::GameToClient::Error(
                                    "Input is not required".to_string(),
                                ),
                                &mut client_interfaces[input.index as usize],
                            )
                        }
                    }

                    Ok("Waiting for players or names")
                }
                // When the number of players is correct and every player is waiting for the
                // server, process the GameSetup event to proceed to CutInitial
                else {
                    if let InputStore::Names(names) = &input_store {
                        game_object
                            .process_event(cribbage::GameEvent::GameSetup {
                                input_player_names: names.clone(),
                                input_manual: man_scoring,
                                input_underscoring: underpegging,
                                input_muggins: muggins,
                                input_overscoring: overpegging,
                            })
                            .unwrap();
                    } else {
                        output = Err("InputStore not Names");
                        break 'game_loop;
                    }
                    Ok("GameSetup event processed")
                }
            }

            // If the GameState is CutInitial, either the GameSetup event has just been processed
            // or the initial cut has failed. If it has failed and not all Confirmations from
            // players have been received (indicated by an input_tracker that is Some) either
            // continue to process Confirmations or reset the input_tracker to None to indicate
            // that the next cut should proceed once all Confirmations from each player has been
            // received. If the input_tracker is None, then the Game is ready to process the
            // Confirmation event and set up the input_tracker to receive Confirmations from each
            // player.
            cribbage::GameState::CutInitial => {
                match &mut input_tracker {
                    // Process the Confirmation event to perform the cuts, then set up the
                    // input_tracker to receive Confirmations from each player in order by index
                    // and send a message asking for confirmation to index 0
                    None => {
                        game_object
                            .process_event(cribbage::GameEvent::Confirmation)
                            .unwrap();

                        input_tracker = Some(OrderedInputTracker {
                            index_active: 0,
                            index_last: None,
                            index_stop: Some(num_players - 1),
                        });

                        client_interfaces[0].state = GciState::WaitingForInitialCut;
                        send_message(
                            super::messages::GameToClient::WaitInitialCut,
                            &mut client_interfaces[0],
                        );

                        Ok("Processed CutInitial and set up input_tracker for receiving Confirmations from each player")
                    }

                    // If all Confirmations have been processed (when index_last of the
                    // input_tracker is equal to index_stop) reset the tracker to None to indicate
                    // that the next CutInitial should proceed. If not all Confirmations have been
                    // processed, procees the incoming inputs
                    Some(ordered_input_tracker) => {
                        if ordered_input_tracker.index_last.is_some()
                            && ordered_input_tracker.index_last == ordered_input_tracker.index_stop
                        {
                            for client_interface in &mut client_interfaces {
                                send_message(
                                    super::messages::GameToClient::InitialCutFailure,
                                    client_interface,
                                )
                            }
                            input_tracker = None;
                            Ok("All Confirmations received, input_tracker reset")
                        } else {
                            // If all players are waiting, then advance the index_active and
                            // index_last and send a WaitInitialCut message to the next client
                            if are_all_players_waiting(&client_interfaces) {
                                ordered_input_tracker.index_last =
                                    Some(ordered_input_tracker.index_active);
                                ordered_input_tracker.index_active += 1;

                                // If not all players have given confirmation, ask the next player
                                // for confirmation
                                if ordered_input_tracker.index_last
                                    != ordered_input_tracker.index_stop
                                {
                                    client_interfaces
                                        [ordered_input_tracker.index_active as usize]
                                        .state = GciState::WaitingForInitialCut;
                                    send_message(
                                        super::messages::GameToClient::WaitInitialCut,
                                        &mut client_interfaces
                                            [ordered_input_tracker.index_active as usize],
                                    )
                                }

                                Ok("Processed Confirmation successfully")
                            }
                            // If there is a player who is not waiting (WaitingForInitialCut should
                            // be the only other state possible), check the client_messages vector
                            // for a Confirmation event from the index_active
                            else {
                                for input in &client_messages {
                                    if input.index == ordered_input_tracker.index_active {
                                        // If a Confirmation event is received from the client
                                        // corresponding to the index_active, announce the cut they
                                        // received and change the state back to WaitingForServer
                                        if input.message
                                            == super::messages::ClientToGame::Confirmation
                                        {
                                            for client_interface in &mut client_interfaces {
                                                send_message( super::messages::GameToClient::InitialCutResult{
                                                    name: game_object.players[input.index as usize].username.clone(),
                                                    card: game_object.players[input.index as usize].hand[0],
                                                }, client_interface)
                                            }

                                            client_interfaces[input.index as usize].state =
                                                GciState::WaitingForServer;
                                        }
                                        // If any other message is received from that client,
                                        // resend the message asking for confirmation
                                        else {
                                            send_message(
                                                super::messages::GameToClient::WaitInitialCut,
                                                &mut client_interfaces[input.index as usize],
                                            )
                                        }
                                    }
                                    // If input is received by someone who is not the index_active,
                                    // send an error indicating that they should not be sending a
                                    // message
                                    else {
                                        send_message(
                                            super::messages::GameToClient::Error(
                                                "Input is not required from you".to_string(),
                                            ),
                                            &mut client_interfaces[input.index as usize],
                                        )
                                    }
                                }
                                Ok("Waiting for Confirmations to continue through the cut")
                            }
                        }
                    }
                }
            }

            // If the GameState is Deal then either the initial cut was successful but the server
            // is still receiving Confirmations and announcing the cut or the server is waiting for
            // Confirmation from the dealer to deal the cards. If the cuts are still being
            // announced, process confirmations as in the CutInitial block, otherwise poll for
            // confirmation from the dealer
            cribbage::GameState::Deal => {
                match &mut input_tracker {
                    // If the input tracker is Some, then the server is still processing Confirmations
                    // for the cut
                    Some(ordered_input_tracker) => {
                        // If the last index is the stop index (ie when all players have given
                        // confirmation) send the InitialCutSuccess message and ask the client who
                        // is dealer to send confirmation
                        if ordered_input_tracker.index_last.is_some()
                            && ordered_input_tracker.index_last == ordered_input_tracker.index_stop
                        {
                            for client_interface in &mut client_interfaces {
                                send_message(
                                    super::messages::GameToClient::InitialCutSuccess(
                                        game_object.players[game_object.index_dealer as usize]
                                            .username
                                            .clone(),
                                    ),
                                    client_interface,
                                )
                            }

                            client_interfaces[game_object.index_dealer as usize].state =
                                GciState::WaitingForDeal;
                            send_message(
                                super::messages::GameToClient::WaitDeal,
                                &mut client_interfaces[game_object.index_dealer as usize],
                            );

                            input_tracker = None;
                            Ok("All Confirmations received, input_tracker reset")
                        } else {
                            // If all players are waiting, then advance the index_active and
                            // index_last and send a WaitInitialCut message to the next client
                            if are_all_players_waiting(&client_interfaces) {
                                ordered_input_tracker.index_last =
                                    Some(ordered_input_tracker.index_active);
                                ordered_input_tracker.index_active += 1;

                                // If not all players have given confirmation, ask the next player
                                // for confirmation
                                if ordered_input_tracker.index_last
                                    != ordered_input_tracker.index_stop
                                {
                                    client_interfaces
                                        [ordered_input_tracker.index_active as usize]
                                        .state = GciState::WaitingForInitialCut;
                                    send_message(
                                        super::messages::GameToClient::WaitInitialCut,
                                        &mut client_interfaces
                                            [ordered_input_tracker.index_active as usize],
                                    );
                                }

                                Ok("Processed Confirmation successfully")
                            }
                            // If there is a player who is not waiting (WaitingForInitialCut should
                            // be the only other state possible), check the client_messages vector
                            // for a Confirmation event from the index_active
                            else {
                                for input in &client_messages {
                                    if input.index == ordered_input_tracker.index_active {
                                        // If a Confirmation event is received from the client
                                        // corresponding to the index_active, announce the cut they
                                        // received and change the state back to WaitingForServer
                                        if input.message
                                            == super::messages::ClientToGame::Confirmation
                                        {
                                            for client_interface in &mut client_interfaces {
                                                send_message(super::messages::GameToClient::InitialCutResult{
                                                    name: game_object.players[input.index as usize].username.clone(),
                                                    card: game_object.players[input.index as usize].hand[0],
                                                }, client_interface)
                                            }

                                            client_interfaces[input.index as usize].state =
                                                GciState::WaitingForServer;
                                        }
                                        // If any other message is received from that client,
                                        // resend the message asking for confirmation
                                        else {
                                            send_message(
                                                super::messages::GameToClient::WaitInitialCut,
                                                &mut client_interfaces[input.index as usize],
                                            )
                                        }
                                    }
                                    // If input is received by someone who is not the index_active,
                                    // send an error indicating that they should not be sending a
                                    // message
                                    else {
                                        send_message(
                                            super::messages::GameToClient::Error(
                                                "Input is not required from you".to_string(),
                                            ),
                                            &mut client_interfaces[input.index as usize],
                                        );
                                    }
                                }
                                Ok("Waiting for Confirmations to continue through the cut")
                            }
                        }
                    }

                    // If the input tracker is None, then the server is waiting for Confirmation from
                    // the dealer to deal the hands; poll for a confirmation event from the dealer
                    // and when it is received process through the deal and sort phases and report
                    // results
                    None => {
                        for input in &client_messages {
                            if input.index == game_object.index_dealer {
                                if input.message == super::messages::ClientToGame::Confirmation {
                                    // Process game through deal
                                    game_object
                                        .process_event(cribbage::GameEvent::Confirmation)
                                        .unwrap();
                                    // Report unsorted hands
                                    send_hands(&game_object, &mut client_interfaces);
                                    // Process game through sort
                                    game_object
                                        .process_event(cribbage::GameEvent::Confirmation)
                                        .unwrap();
                                    // Report sorted hands
                                    send_hands(&game_object, &mut client_interfaces);

                                    // Set up clients for discard selection
                                    for client_interface in &mut client_interfaces {
                                        input_store = InputStore::Discards(Vec::new());

                                        if num_players != 5
                                            || client_interface.index
                                                != Some(game_object.index_dealer)
                                        {
                                            client_interface.state = GciState::WaitingForDiscards;

                                            // Ask for two discards when there are two players and
                                            // one discard when there are three or more players
                                            if num_players == 2 {
                                                send_message(
                                                    super::messages::GameToClient::WaitDiscardTwo,
                                                    client_interface,
                                                );
                                            } else {
                                                send_message(
                                                    super::messages::GameToClient::WaitDiscardOne,
                                                    client_interface,
                                                );
                                            }
                                        }
                                        // If there are five players and the client is the dealer,
                                        // they do not discard a card
                                        else {
                                            client_interface.state = GciState::WaitingForServer;
                                        }
                                    }
                                }
                                // If the dealer sends a message other than confirmation, resend
                                // the WaitDeal message
                                else {
                                    send_message(
                                        super::messages::GameToClient::WaitDeal,
                                        &mut client_interfaces[input.index as usize],
                                    );
                                }
                            }
                            // Send an Error message to any players who send a message beside the
                            // dealer
                            else {
                                send_message(
                                    super::messages::GameToClient::Error(
                                        "Input is not required form you".to_string(),
                                    ),
                                    &mut client_interfaces[input.index as usize],
                                );
                            }
                        }

                        Ok("Waiting for confirmation from dealer")
                    }
                }
            }

            // If the GameState is Discard, then the game is waiting for DiscardTwo (when two
            // players) or DiscardOne (three to five players) messages.
            cribbage::GameState::Discard => {
                // If all players are waiting, construct the DiscardSelection event from the
                // input_store and progress the game
                if are_all_players_waiting(&client_interfaces) {
                    // Prepare vector to be used in process_event
                    let mut discards: Vec<Vec<cribbage::deck::Card>> = Vec::new();
                    for _ in 0..num_players {
                        discards.push(Vec::new());
                    }

                    if let InputStore::Discards(selections) = &input_store {
                        for selection in selections {
                            match selection {
                                DiscardSelection::OneDiscard {
                                    player_index,
                                    card_index,
                                } => {
                                    discards[*player_index as usize].push(
                                        game_object.players[*player_index as usize].hand
                                            [*card_index as usize],
                                    );
                                }
                                DiscardSelection::TwoDiscard {
                                    player_index,
                                    card_index_one,
                                    card_index_two,
                                } => {
                                    discards[*player_index as usize].push(
                                        game_object.players[*player_index as usize].hand
                                            [*card_index_one as usize],
                                    );

                                    discards[*player_index as usize].push(
                                        game_object.players[*player_index as usize].hand
                                            [*card_index_two as usize],
                                    );
                                }
                            }
                        }
                    }

                    game_object
                        .process_event(cribbage::GameEvent::DiscardSelection(discards))
                        .unwrap();

                    for client_interface in &mut client_interfaces {
                        send_message(super::messages::GameToClient::AllDiscards, client_interface);
                    }

                    Ok("Proceeded through Discard")
                }
                // If there are any players not waiting, poll for DiscardOne or DiscardTwo messages
                else {
                    for input in &mut client_messages {
                        if client_interfaces[input.index as usize].state
                            == GciState::WaitingForDiscards
                        {
                            // Poll for DiscardTwo events when there are two players
                            if num_players == 2 {
                                if let super::messages::ClientToGame::DiscardTwo {
                                    index_one,
                                    index_two,
                                } = input.message
                                {
                                    // Add DiscardSelection to input_store
                                    if let InputStore::Discards(discards) = &mut input_store {
                                        discards.push(DiscardSelection::TwoDiscard {
                                            player_index: input.index,
                                            card_index_one: index_one,
                                            card_index_two: index_two,
                                        });
                                    }
                                    // Abort from game if the InputStore is not Discards
                                    else {
                                        output = Err("InputStore is not Discards");
                                        break 'game_loop;
                                    }

                                    // Announce that the discards were placed
                                    for client_interface in &mut client_interfaces {
                                        println!("Sent DiscardPlacedTwo");
                                        send_message(
                                            super::messages::GameToClient::DiscardPlacedTwo(
                                                game_object.players[input.index as usize]
                                                    .username
                                                    .clone(),
                                            ),
                                            client_interface,
                                        );
                                    }

                                    // Change the player's state to WaitingForServer
                                    client_interfaces[input.index as usize].state =
                                        GciState::WaitingForServer;
                                }
                                // If the message is not a DiscardTwo, send the WaitDiscardTwo
                                // message
                                else {
                                    send_message(
                                        super::messages::GameToClient::WaitDiscardTwo,
                                        &mut client_interfaces[input.index as usize],
                                    );
                                }
                            }
                            // Poll for DiscardOne events when there are three to five players
                            else {
                                if let super::messages::ClientToGame::DiscardOne { index } =
                                    input.message
                                {
                                    // Add DiscardSelection to input_store
                                    if let InputStore::Discards(discards) = &mut input_store {
                                        discards.push(DiscardSelection::OneDiscard {
                                            player_index: input.index,
                                            card_index: index,
                                        });
                                    }
                                    // Abort from game if the InputStore is not Discards
                                    else {
                                        output = Err("InputStore is not Discards");
                                        break 'game_loop;
                                    }

                                    // Announce that the discards were placed
                                    for client_interface in &mut client_interfaces {
                                        send_message(
                                            super::messages::GameToClient::DiscardPlacedOne(
                                                game_object.players[input.index as usize]
                                                    .username
                                                    .clone(),
                                            ),
                                            client_interface,
                                        );
                                    }

                                    // Change the player's state to WaitingForServer
                                    client_interfaces[input.index as usize].state =
                                        GciState::WaitingForServer;
                                }
                                // If the message is not a DiscardOne, send a WaitDiscardOne
                                else {
                                    send_message(
                                        super::messages::GameToClient::WaitDiscardOne,
                                        &mut client_interfaces[input.index as usize],
                                    );
                                }
                            }
                        } else {
                            send_message(
                                super::messages::GameToClient::Error(
                                    "Input is not required from you.".to_string(),
                                ),
                                &mut client_interfaces[input.index as usize],
                            );
                        }
                    }
                    Ok("Polling for DiscardOne or DiscardTwo messages")
                }
            }

            // Prepare the game to shutdown when the end state is reached
            cribbage::GameState::End => Ok("Server ending"),

            // Return an error for any
            _ => Err("Unrecognized GameState"),
        };

        client_messages.clear();
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
            .send(super::messages::GameToClient::Disconnect)
            .unwrap();

        assert!(
            client_interface.receiver.recv()
                == Ok(super::messages::ClientToGame::TransmissionReceived)
        );
    }

    main_transmitter
        .send(super::messages::GameToMain::EndServer)
        .unwrap();
}
