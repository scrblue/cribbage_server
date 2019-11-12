extern crate cribbage;
extern crate serde;
use serde::{Deserialize, Serialize};
use std::sync::mpsc;

// Messages from the client handler threads to the game model thread; also the messages sent from
// the client to the client handler thread over TCP
#[derive(PartialEq, Serialize, Deserialize)]
pub enum ClientToGame {
    // A message to initiate communication between the client thread and the game thread and to
    // indicate that the client thread is ready to receive requests
    Greeting,

    // A simple confirmation from the client to continue the game model progression
    Confirmation,

    // The name the client wishes to be known by for the duration of the game
    // TODO A way to determine this with user authentication for eventual lobby and account system
    Name(String),
}

// Messages sent from the game model to the client handler threads which more directly interact
// with the players; also the messages sent from the client handler to the client over TCP
#[derive(Serialize, Deserialize)]
pub enum GameToClient {
    // Message indicating that the maximum number of cliets that can play have already joined
    DeniedTableFull,

    // That the game model is requesting the name that the client will go by
    WaitName,

    // That a player has successfully joined the game
    PlayerJoinNotification {
        name: String,
        number: u8,
        of: u8,
    },

    // That the model is waiting for a confirmation event to process that player's initial cut
    WaitInitialCut,

    // That the named player has cut the specified card in their initial cut
    InitialCutResult {
        name: String,
        card: cribbage::deck::Card,
    },

    // That the named player has been decided to be the first dealer as per the cut
    InitialCutSuccess(String),

    // That the cut has resulted in a tie and that it must be redone
    InitialCutFailure,

    // That an error has occured
    Error(String),

    // That the client should not expect further messages from the game model
    Disconnect,
}

pub enum GameToMain {
    // That the main thread is ready to end as the match has finished
    EndServer,
}

pub enum MainToGame {
    // Message handing the transmitter and receiver from/to a new client thread to/from the game
    // model thread
    NewClient {
        transmitter: mpsc::Sender<GameToClient>,
        receiver: mpsc::Receiver<ClientToGame>,
    },
}
