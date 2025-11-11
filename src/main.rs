mod journal;

use journal::{Logger};

fn main() -> std::io::Result<()> {
    let mut logger = Logger::new("journal.log")?;

    logger.log("SEND", "NodeB", "Hello")?;
    logger.log("RECV", "NodeC", "Ack")?;
    logger.log("SEND", "NodeD", "Next message")?;

    Ok(())
}
