use book::err_utils::ErrStr;
use quizzes::quiz02::d_gemynd::runoff_with_args;

#[tokio::main]
async fn main() -> ErrStr<()> { runoff_with_args().await }
