use book::err_utils::ErrStr;
use quizzes::quiz01::a_tva::runoff_with_args;

#[tokio::main]
async fn main() -> ErrStr<()> {
    runoff_with_args().await
}
