use book::err_utils::ErrStr;
use quizzes::quiz01::c_maegen::runoff_with_args;

#[tokio::main] async fn main() -> ErrStr<()> { runoff_with_args().await }
