use quizzes::quiz02::c_ceap::runoff_with_args;
use book::err_utils::ErrStr;

#[tokio::main]
async fn main() -> ErrStr<()> {
    runoff_with_args().await
}
