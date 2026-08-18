use quizzes::quiz02::b_frignan::runoff_with_args;
use book::err_utils::ErrStr;

#[tokio::main]
async fn main() -> ErrStr<()> {
    runoff_with_args().await
}
