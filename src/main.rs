mod journal;
use journal::LogType;
use journal::Logger;

fn main() -> std::io::Result<()> {
    let mut logger = Logger::new("journal.log", 5000, 200)?;

    logger.log(LogType::Send, 42, "Salut je suis une base64")?;
    logger.log(LogType::Recv, 69, "Salut je suis une base64")?;

    Ok(())
}
