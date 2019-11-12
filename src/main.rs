extern crate cribbage;
extern crate serde;
mod client;
mod game;
mod messages;
use std::env;
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
    let mut desired_num_players: u8 = args[2].trim().parse().unwrap();
    let mut active_num_players: u8 = 0;

    // Parses the boolean options given in the command line arguments
    let man_score: bool = str::FromStr::from_str(&args[3]).unwrap();
    let underpegging: bool = str::FromStr::from_str(&args[4]).unwrap();
    let muggins: bool = str::FromStr::from_str(&args[5]).unwrap();
    let overpegging: bool = str::FromStr::from_str(&args[6]).unwrap();

    let listener = net::TcpListener::bind(&address).unwrap();

    // The game object to be used on the server
    let mut game = cribbage::Game::new();

    let (game_handler_to_main_transmitter, game_handler_to_main_receiver) = mpsc::channel();
    let (main_to_game_handler_transmitter, main_to_game_handler_receiver) = mpsc::channel();

    thread::spawn(move || {
        game::handle_game(
            game,
            desired_num_players.clone(),
            man_score,
            underpegging,
            muggins,
            overpegging,
            main_to_game_handler_receiver,
            game_handler_to_main_transmitter,
        );
    });

    println!("Waiting for connection on ip {}", address);

    // Forward each cliet to the cliet handler
    'listen_loop: for mut stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                // FIXME Error exiting main thread
                // Check for the end signal
                match game_handler_to_main_receiver.try_recv() {
                    Ok(messages::GameToMain::EndServer) => {
                        break 'listen_loop;
                    }
                    _ => {}
                }

                // Creates the transmitter and receivers used by the game model to communicate with the
                // client
                let (
                    client_handler_to_game_handler_transmitter,
                    client_handler_to_game_handler_receiver,
                ) = mpsc::channel();
                let (
                    game_handler_to_client_handler_transmitter,
                    game_handler_to_client_handler_receiver,
                ) = mpsc::channel();

                thread::spawn(move || {
                    client::handle_client(
                        stream,
                        client_handler_to_game_handler_transmitter,
                        game_handler_to_client_handler_receiver,
                    );
                });

                // Send the new transmitter for the messages from the game model thread to the
                // new client thread to the game model
                main_to_game_handler_transmitter
                    .send(messages::MainToGame::NewClient {
                        transmitter: game_handler_to_client_handler_transmitter,
                        receiver: client_handler_to_game_handler_receiver,
                    })
                    .unwrap();

                active_num_players += 1;
            }
            Err(e) => {
                println!("Error accepting connection to client; {}", e);
            }
        }
    }

    println!("Exiting server");
}
