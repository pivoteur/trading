use solonge::paths;

fn main() {
    let path = paths::tsv_url("asdf: asdf", "tsv: auto_trading.log");
    println!("Path: {}", path);
}