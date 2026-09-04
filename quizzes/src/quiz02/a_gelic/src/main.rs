use book::err_utils::ErrStr;
use quizzes::quiz02::a_gelic::runoff_with_args;

#[tokio::main]
async fn main() -> ErrStr<()> { runoff_with_args().await }
