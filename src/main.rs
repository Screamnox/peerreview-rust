mod journal;
use journal::LogType;
use journal::Logger;

fn main() -> std::io::Result<()> {
    let mut logger = Logger::new("journal.log", 10, 200)?;

    logger.log(LogType::Send, 42, "Salut je suis une base64")?;
    logger.log(LogType::Recv, 69, "Salut je suis une base64")?;

    let result = logger.get_log(100)?;

    println!("Taille de get_log : {}", result.len());

    Ok(())
}
