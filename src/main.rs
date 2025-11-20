mod journal;

use journal::{Logger};
use journal::{LogType};

fn main() -> std::io::Result<()> {
    let mut logger = Logger::new("journal.log",5000,200)?;

    logger.log(LogType::SEND, 42, "Salut je suis une base64")?;
    logger.log(LogType::RECV, 69, "Salut je suis une base64")?;
    
    


    Ok(())
}
