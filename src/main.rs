use shadow_harvester_lib::scavenge;

fn main() {
    // --- API Test Vector Data (Placeholder) ---
    let my_registered_address = "addr_test1qq4dl3nhr0axurgcrpun9xyp04pd2r2dwu5x7eeam98psv6dhxlde8ucclv2p46hm077ds4vzelf5565fg3ky794uhrq5up0he".to_string();
    let challenge_id = "**D07C10".to_string();
    let difficulty = "000FFFFF".to_string();
    let no_pre_mine_key = "fd651ac2725e3b9d804cc8b161c0709af14d6264f93e8d4afef0fd1142a3f011".to_string();
    let latest_submission = "2025-10-19T08:59:59.000Z".to_string();
    let no_pre_mine_hour = "509681483".to_string();
    let nb_threads = 10;

    scavenge(
        my_registered_address,
        challenge_id,
        difficulty,
        no_pre_mine_key,
        latest_submission,
        no_pre_mine_hour,
        nb_threads
    );
}
