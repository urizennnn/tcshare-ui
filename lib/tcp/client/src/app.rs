use std::error::Error;
use tokio::{net::TcpListener, net::TcpStream};
use whoami::username;

use std::sync::LazyLock;

pub static USER: LazyLock<String> = LazyLock::new(|| match username() {
    Ok(value) => value,
    Err(e) => {
        log::error!("Failed to get username: {}", e);
        "unknown_user".to_string()
    }
});
pub fn connect_sync(address: &str) -> Result<TcpStream, Box<dyn Error>> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let stream = TcpStream::connect(address).await?;
        Ok(stream)
    })
}
/// Asynchronously binds a TCP listener to port 4239 on all network interfaces.
///
/// # Returns
/// A `TcpListener` bound to "0.0.0.0:4239" on success, or a boxed error if binding fails.
pub async fn listen() -> Result<TcpListener, Box<dyn std::error::Error + Send>> {
    match TcpListener::bind("0.0.0.0:4239").await {
        Ok(listener) => Ok(listener),
        Err(e) => {
            log::error!("Failed to bind: {}", e);
            Err(Box::new(e))
        }
    }
}

// pub async fn start() -> Result<(), Box<dyn Error>> {
//     let mut stream = TcpStream::connect("localhost:8080").await.unwrap();
//     stream.write_all(USER.as_bytes()).await?;
//     stream.flush().await?;
//
//     info!("Connected to server");
//     init().await?;
//     let mut buffer = vec![0; 5_242_880];
//
//     loop {
//         let mut input = String::new();
//         io::stdin().lock().read_line(&mut input)?;
//         let input = input.trim().to_string();
//
//         match input.split_whitespace().next() {
//             Some("EXIT") => {
//                 info!("Exiting");
//                 break;
//             }
//             Some("PUT") => {
//                 let parts: Vec<&str> = input.split_whitespace().collect();
//                 if parts.len() != 2 {
//                     error!("Invalid PUT command. Usage: PUT <filename>");
//                     continue;
//                 }
//                 stream.write_all(input.as_bytes()).await?;
//                 stream.flush().await?;
//                 update_init(parts[1].to_string()).await?;
//                 upload(&mut stream, parts[1], &mut buffer).await?;
//             }
//             Some("LIST") => {
//                 stream.write_all(input.as_bytes()).await?;
//                 stream.flush().await?;
//                 list(&mut stream, &mut buffer).await?;
//             }
//             Some("GET") => {
//                 let parts: Vec<&str> = input.split_whitespace().collect();
//                 let dest: Vec<&str> = input.split_whitespace().collect();
//
//                 if parts.len() != 3 {
//                     error!("Invalid GET command. Usage: GET <filename> <destination-path>");
//                     continue;
//                 }
//                 stream.write_all(input.as_bytes()).await?;
//                 stream.flush().await?;
//
//                 // Read server response
//                 let n = stream.read(&mut buffer).await?;
//                 let response = String::from_utf8_lossy(&buffer[..n]).trim().to_string();
//                 info!("Received response: {}", response);
//
//                 if response.starts_with("SEND") {
//                     let parts: Vec<&str> = response.split_whitespace().collect();
//                     receive_files(&mut stream, &mut buffer, &parts, &dest).await?;
//                 } else {
//                     error!("File not found on server");
//                 }
//             }
//             Some("DELETE") => {
//                 let parts: Vec<&str> = input.split_whitespace().collect();
//                 if parts.len() != 2 {
//                     error!("Invalid DELETE command. Usage: DELETE <filename>");
//                     continue;
//                 }
//                 stream.write_all(input.as_bytes()).await?;
//                 stream.flush().await?;
//
//                 // Read server response
//                 let n = stream.read(&mut buffer).await?;
//                 let response = String::from_utf8_lossy(&buffer[..n]).trim().to_string();
//                 info!("Received response: {}", response);
//
//                 // Add DELETE functionality here
//             }
//             _ => {
//                 warn!("Invalid command. Please try again.");
//             }
//         }
//     }
//     Ok(())
// }
