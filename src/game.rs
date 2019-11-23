extern crate cribbage;
use std::sync::mpsc;
use std::{thread, time};

#[derive(PartialEq, Debug)]
enum GciState {
    // The state given to a client that has just connected and who has yet to send the greeting
    Connecting,

    // Any clients that are denied because the table is full
    Watching,

    // Any players who have joined the table and who have been asked for a name
    WaitingName,

    // Players who have already given their name and who are waiting for other players to do the
    // same
    WaitingOtherNames,

    // The player who has been requested confirmation for the cut
    WaitingForInitialCut,

    // The players who have already given confirmation or who have yet to be asked for their
    // confirmation for their cuts
    WaitingForOtherInitialCuts,

    // The dealer who's confirmation deals the hands
    WaitingForDeal,

    // The dealer who's confirmation for dealing the hand has been received
    Dealt,

    // When the game has finished
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

// A structure used to forward ClientToGame messages from the receiver in the GameClientInterface
// from the loop polling the receiver to the logic of the game such that a mutable reference to the
// client_interfaces is not already used
struct ClientMessage {
    index: u8,
    message: super::messages::ClientToGame,
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

    // A variable tracking the player index that was last requested input eg. the confirmation call
    // when dealing with the initial, the index that last responded,  the index that it started
    // with, and the index it's stopping at. 0 and num_players - 1 requires all players to provide
    // input in order, index_dealer + 1 to index dealer is the first pone to the dealer in order,
    // a single index for both is requesting input from one player only
    let mut active_index: Option<u8> = None;
    let mut last_index: Option<u8> = None;
    let mut start_index: Option<u8> = None;
    let mut stop_index: Option<u8> = None;

    // A variable tracking the number of clients that are also players; less than or equal to the
    // number of players
    let mut num_connected_players: u8 = 0;

    // A vector holding a list of messages to be sent to every client
    let mut announcement_list: Vec<super::messages::GameToClient> = Vec::new();

    // A vector of strings holding player names to be fed to the game_object; loop is to create an
    // empty string for each name so they can be filled out of order
    let mut names: Vec<String> = Vec::new();
    for _ in 0..num_players {
        names.push(String::new());
    }

    // A variable holding the output of the game loop
    let mut output: Result<&str, &str> = Ok("Game thread running");

    // While the output of the game model is valid
    while output.is_ok() && output != Ok("Server ending") {
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
                            client_interface
                                .transmitter
                                .send(super::messages::GameToClient::WaitName)
                                .unwrap();
                            num_connected_players += 1;
                        } else {
                            println!("New watcher");
                            client_interface.state = GciState::Watching;
                            client_interface
                                .transmitter
                                .send(super::messages::GameToClient::DeniedTableFull)
                                .unwrap();
                        }
                    }
                }
                // If the client sends a Name message during the GameStart GameState and the
                // WaitName GciState, add the name to the vector and announce the name to the
                // players
                Ok(super::messages::ClientToGame::Name(name)) => {
                    println!("Received Name");
                    if client_interface.state == GciState::WaitingName
                        && game_object.state == cribbage::GameState::GameStart
                    {
                        names[client_interface.index.unwrap() as usize] = name;
                        client_interface.state = GciState::WaitingOtherNames;
                        announcement_list.push(
                            super::messages::GameToClient::PlayerJoinNotification {
                                name: names[client_interface.index.unwrap() as usize].clone(),
                                number: client_interface.index.unwrap() + 1,
                                of: num_players,
                            },
                        );
                    }
                }

                // If the cliet sends confirmation under various conditions
                Ok(super::messages::ClientToGame::Confirmation) => {
                    // If the client with the active index sends confirmation when GciState is
                    // WaitingForInitialCut, add their cut to the announcements
                    // Unwrap or is 6 and 7 as a maximum of 5 players can play and the condition
                    // should not resolve to true if either value is None
                    if client_interface.index.unwrap_or(6) == active_index.unwrap_or(7)
                        && client_interface.state == GciState::WaitingForInitialCut
                    {
                        announcement_list.push(super::messages::GameToClient::InitialCutResult {
                            name: game_object.players[client_interface.index.unwrap() as usize]
                                .username
                                .clone(),
                            card: game_object.players[client_interface.index.unwrap() as usize]
                                .hand[0],
                        });
                        client_interface.state = GciState::WaitingForOtherInitialCuts;
                        last_index = active_index;
                        active_index = Some(active_index.unwrap() + 1);
                    }

                    // If the client game index is the index of the dealer (if the client is the
                    // dealer_ and the GciState is WaitingForDeal, chang the state to dealt and
                    // notify players that cards are being dealt
                    if client_interface.index.unwrap_or(6) == game_object.index_dealer
                        && client_interface.state == GciState::WaitingForDeal
                    {
                        announcement_list.push(super::messages::GameToClient::Dealing);
                        client_interface.state = GciState::Dealt;
                    }
                }

                _ => {}
            }
        }

        for client_interface in &mut client_interfaces {
            // Send all announcements that were not added in this loop of the game loop
            for announcement in &announcement_list {
                println!("Making annoucement");
                client_interface
                    .transmitter
                    .send(announcement.clone())
                    .unwrap();
            }
        }

        announcement_list.clear();

        // Processes that occur after player input has been dealt with

        // When GameState is GameStart, the proper number of players have connected, and all
        // players states are WaitingOtherNames, progress through the CutInitial and send the
        // WaitInitialCut message to player index 0
        if num_connected_players == num_players
            && game_object.state == cribbage::GameState::GameStart
        {
            let mut is_waiting_other_names = true;
            for client_interface in &client_interfaces {
                if client_interface.index.is_some()
                    && client_interface.state != GciState::WaitingOtherNames
                {
                    is_waiting_other_names = false;
                }
            }

            if is_waiting_other_names {
                game_object
                    .process_event(cribbage::GameEvent::GameSetup {
                        input_player_names: names.clone(),
                        input_manual: man_scoring,
                        input_underscoring: underpegging,
                        input_muggins: muggins,
                        input_overscoring: overpegging,
                    })
                    .unwrap();

                // Progress the game through the first initial; if there is a tie the cut is redone
                game_object
                    .process_event(cribbage::GameEvent::Confirmation)
                    .unwrap();

                // Set the active index to zero and send the WaitInitialCut to player index 0
                active_index = Some(0);
                start_index = Some(0);
                stop_index = Some(num_players - 1);
                for client_interface in &mut client_interfaces {
                    if client_interface.index == Some(0) {
                        client_interface
                            .transmitter
                            .send(super::messages::GameToClient::WaitInitialCut)
                            .unwrap();
                        client_interface.state = GciState::WaitingForInitialCut;
                    } else {
                        client_interface.state = GciState::WaitingForOtherInitialCuts;
                    }
                }
            }
        }

        // When all the players' states are WaitingForOtherInitialCuts and the last index is
        // below the stop index (ie when someone has given their confirmation and the
        // active index has incremented), change the state of the client interface with the
        // index corresponding to the active index to WaitingForInitialCut to indicate that
        // their confirmation is now requested
        if game_object.state == cribbage::GameState::CutInitial
            || game_object.state == cribbage::GameState::Deal
        {
            let mut is_waiting_for_other_cuts = true;
            for client_interface in &client_interfaces {
                if client_interface.index.is_some()
                    && client_interface.state != GciState::WaitingForOtherInitialCuts
                {
                    is_waiting_for_other_cuts = false;
                }
            }

            if is_waiting_for_other_cuts && last_index != stop_index {
                for client_interface in &mut client_interfaces {
                    if client_interface.index == active_index {
                        println!("Index is active");
                        client_interface.state = GciState::WaitingForInitialCut;
                        thread::sleep(time::Duration::from_secs(1));
                        client_interface
                            .transmitter
                            .send(super::messages::GameToClient::WaitInitialCut)
                            .unwrap();
                    } else {
                        println!("Index isn't active");
                    }
                }
            }
        }

        // When GameState is CutInitial and the last index is equal to the number of players - 1
        // (ie when the cut must be repeated and and the all players from the previous cut have
        // confirmed), redo the cut
        if game_object.state == cribbage::GameState::CutInitial && last_index == stop_index {
            for client_interface in &mut client_interfaces {
                client_interface
                    .transmitter
                    .send(super::messages::GameToClient::InitialCutFailure)
                    .unwrap();
            }
            game_object
                .process_event(cribbage::GameEvent::Confirmation)
                .unwrap();

            // Set the indices and send the WaitInitialCut to player index 0
            active_index = Some(0);
            start_index = Some(0);
            stop_index = Some(num_players - 1);
            last_index = None;

            for client_interface in &mut client_interfaces {
                if client_interface.index == Some(0) {
                    client_interface
                        .transmitter
                        .send(super::messages::GameToClient::WaitInitialCut)
                        .unwrap();
                }
            }
        }

        // When GameState is Deal and all last index is equal to the number of players - 1(ie when
        // the cut was successful and all players from the previous cut have confirmed) send notice
        // to dealer to deal the cards
        if game_object.state == cribbage::GameState::Deal
            && last_index.is_some()
            && last_index == stop_index
        {
            for client_interface in &mut client_interfaces {
                client_interface
                    .transmitter
                    .send(super::messages::GameToClient::InitialCutSuccess(
                        game_object.players[game_object.index_dealer as usize]
                            .username
                            .clone(),
                    ))
                    .unwrap();
                if client_interface.index == Some(game_object.index_dealer) {
                    client_interface.state = GciState::WaitingForDeal;
                    thread::sleep(time::Duration::from_secs(1));
                    client_interface
                        .transmitter
                        .send(super::messages::GameToClient::WaitDeal)
                        .unwrap();
                }
            }

            active_index = None;
            last_index = None;
            start_index = None;
            stop_index = None;
        }

        // When the GameState is Deal and the dealer's GciState is Dealt, process the game object
        // and announce the cards, sort them and announce the sorted deck, and end the game
        if game_object.state == cribbage::GameState::Deal {
            // Determine if confirmation has been sent and process through the deal
            let mut is_dealt = false;
            for client_interface in &client_interfaces {
                if client_interface.index == Some(game_object.index_dealer)
                    && client_interface.state == GciState::Dealt
                {
                    is_dealt = true;
                    game_object
                        .process_event(cribbage::GameEvent::Confirmation)
                        .unwrap();
                }
            }

            if is_dealt {
                thread::sleep(time::Duration::from_secs(1));

                // Send unsorted hands
                for client_interface in &mut client_interfaces {
                    if client_interface.index.is_some() {
                        client_interface
                            .transmitter
                            .send(super::messages::GameToClient::DealtHand(
                                game_object.players[client_interface.index.unwrap() as usize]
                                    .hand
                                    .clone(),
                            ))
                            .unwrap();
                    }
                }

                thread::sleep(time::Duration::from_secs(1));

                // Sort the hands
                game_object
                    .process_event(cribbage::GameEvent::Confirmation)
                    .unwrap();

                // Send sorted hands
                for client_interface in &mut client_interfaces {
                    if client_interface.index.is_some() {
                        client_interface
                            .transmitter
                            .send(super::messages::GameToClient::DealtHand(
                                game_object.players[client_interface.index.unwrap() as usize]
                                    .hand
                                    .clone(),
                            ))
                            .unwrap();
                    }
                }

                output = Ok("Server ending");
            }
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
            .send(super::messages::GameToClient::Disconnect)
            .unwrap();
    }

    main_transmitter
        .send(super::messages::GameToMain::EndServer)
        .unwrap();
}
