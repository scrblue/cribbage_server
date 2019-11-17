extern crate cribbage;
extern crate serde;
mod client;
mod game;
mod messages;
use std::env;
use std::io;
use std::net;
use std::str;
use std::sync::mpsc;
use std::thread;

fn main() {
    // Command line arguments are a port number, the number of players to wait for, and whether or
    // not manual scoring, underscoring, muggins, and overscoring are enabled in that order (sample
    // input is "executable_name 1025 2 false false false false"); lobby will launch a server
    // instance on a free port and direct players to it

    let args: Vec<String> = env::args().collect();

    // Adds the port to listen to on to the local IP
    let mut address = "127.0.0.1:".to_string();
    address.push_str(&args[1]);

    // Parses the number of players to wait for
    let desired_num_players: u8 = args[2].trim().parse().unwrap();

    // Parses the boolean options given in the command line arguments
    let man_score: bool = str::FromStr::from_str(&args[3]).unwrap();
    let underpegging: bool = str::FromStr::from_str(&args[4]).unwrap();
    let muggins: bool = str::FromStr::from_str(&args[5]).unwrap();
    let overpegging: bool = str::FromStr::from_str(&args[6]).unwrap();

    // The TCP listener to form connections
    let listener = net::TcpListener::bind(&address).unwrap();
    listener
        .set_nonblocking(true)
        .expect("Can not set listener non-blocking");

    // The game object to be used on the server
    let mut game = cribbage::Game::new();

    let (game_handler_to_main_transmitter, game_handler_to_main_receiver) = mpsc::channel();
    let (main_to_game_handler_transmitter, main_to_game_handler_receiver) = mpsc::channel();

    thread::spawn(move || {
        game::handle_game(
            game,
            desired_num_players,
            man_score,
            underpegging,
            muggins,
            overpegging,
            main_to_game_handler_receiver,
            game_handler_to_main_transmitter,
        );
    });

    println!("Waiting for connection on ip {}", address);

    let mut game_over = false;
    while !game_over {
        // Continuously poll for an end server event
        game_over = match game_handler_to_main_receiver.try_recv() {
            Ok(messages::GameToMain::EndServer) => true,
            _ => false,
        };
        // And poll for a new connection to the listener
        match listener.accept() {
            Ok((socket, address)) => {
                // Creates the transmitters and receivers used by the game model to communicate
                // with each client handler thread
                let (
                    client_handler_to_game_handler_transmitter,
                    client_handler_to_game_handler_receiver,
                ) = mpsc::channel();
                let (
                    game_handler_to_client_handler_transmitter,
                    game_handler_to_client_handler_receiver,
                ) = mpsc::channel();

                // Spawns the client handler thread and gives ownership of the proper transmitter
                // and receiver
                thread::spawn(move || {
                    client::handle_client(
                        socket,
                        client_handler_to_game_handler_transmitter,
                        game_handler_to_client_handler_receiver,
                    );
                });

                // Send the other transmitter and receiver to the game model thread
                main_to_game_handler_transmitter
                    .send(messages::MainToGame::NewClient {
                        transmitter: game_handler_to_client_handler_transmitter,
                        receiver: client_handler_to_game_handler_receiver,
                    })
                    .unwrap();

                println!("Connected to client on {}", address);
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e) => {
                println!("{}", e);
            }
        };
    }

    println!("Exiting server");
}
