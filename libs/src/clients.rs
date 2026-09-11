//============================================================================
//----- Shared HTTP Client ----------------------------------------------------
//============================================================================

const HTTP_TIMEOUT_SECS: u64 = 15;

pub fn http_client() -> ErrStr<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("Could not build HTTP client: {e}"))
}

