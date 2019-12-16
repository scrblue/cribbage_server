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

    // A simple denial for when the player is given a yes/no choice
    Denial,

    // The name the client wishes to be known by for the duration of the game
    // TODO A way to determine this with user authentication for eventual lobby and account system
    Name(String),

    // The index or indices given are to be discarded
    DiscardOne { index: u8 },
    DiscardTwo { index_one: u8, index_two: u8 },

    // That a given index has been played; as a hand is four cards, an index of 0 to 3 will
    // represent that card being played and a None will represent a go
    PlayTurn(Option<u8>),

    // That the included ScoreEvents have been given by the player for the most recent play
    PlayScore(Vec<cribbage::score::ScoreEvent>),

    TransmissionReceived,
}

// Messages sent from the game model to the client handler threads which more directly interact
// with the players; also the messages sent from the client handler to the client over TCP
#[derive(Serialize, Deserialize, Clone)]
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

    // That the game is waiting for confirmation to deal the hand
    WaitDeal,

    // That cards are actively being dealt
    Dealing,

    // That the player's hand is the included vector
    DealtHand(Vec<cribbage::deck::Card>),

    // That the game is waiting for a discard selection of one card
    WaitDiscardOne,
    // That the game is waiting for a discard selection of two cards
    WaitDiscardTwo,

    // That someone has discarded their card
    DiscardPlacedOne(String),
    // That someone has discarded their cards
    DiscardPlacedTwo(String),

    // That all discards have been placed
    AllDiscards,

    // That the game is waiting for confirmation to cut the starter card
    WaitCutStarter,

    // That the starter card has been cut and the name of the player who cut it and its value
    CutStarter(String, cribbage::deck::Card),

    // That the game is waiting to know whether the dealer calls nibs or not
    WaitNibs,

    // That the dealer has cut a jack and received two points
    Nibs,

    // That a player has played a card and the following ScoreEvents have been claimed
    CardPlayed {
        name: String,
        card: cribbage::deck::Card,
        scores: Vec<cribbage::score::ScoreEvent>,
    },

    // That the game is waiting for a player to place a card and that the valid indices are as
    // listed
    WaitPlay(Vec<u8>),

    // That the game is waiting for ScoreEvents for the previous play
    WaitPlayScore,

    // That the game rejected the scoring because there was an invalid ScoreEvent
    InvalidPlayScoring,

    // That the game has rejected the scoring because the scores are incomplete
    IncompletePlayScoring,

    // That the scores are as follows; contains a vector of pairs of names and scores
    ScoreUpdate(Vec<(String, u8)>),

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
