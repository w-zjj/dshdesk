use serde_json::Value;

const GITHUB_API: &str = "https://api.github.com/repos/w-zjj/dshdesk/releases/latest";
const USER_AGENT: &str = "dshdesk-update-checker";

pub struct UpdateInfo {
    pub tag: String,
    pub html_url: String,
    pub body: String,
}

// 查询 GitHub Releases 最新版。网络失败静默返回 None，不阻塞用户使用
pub fn check_latest() -> Option<UpdateInfo> {
    let body = ureq::get(GITHUB_API)
        .set("User-Agent", USER_AGENT)
        .call()
        .ok()?
        .into_string()
        .ok()?;
    let json: Value = serde_json::from_str(&body).ok()?;

    let tag = json["tag_name"].as_str()?.to_string();
    let html_url = json["html_url"].as_str()?.to_string();
    let body = json["body"].as_str().unwrap_or("").to_string();
    Some(UpdateInfo { tag, html_url, body })
}

// 简单语义版本比较：v1.0.1 vs v1.0.0 → true
pub fn is_newer(remote: &str, local: &str) -> bool {
    let parse = |s: &str| -> Vec<u32> {
        s.trim_start_matches('v')
            .split('.')
            .filter_map(|p| p.parse().ok())
            .collect()
    };
    let r = parse(remote);
    let l = parse(local);
    for i in 0..r.len().max(l.len()) {
        let rv = r.get(i).copied().unwrap_or(0);
        let lv = l.get(i).copied().unwrap_or(0);
        if rv > lv {
            return true;
        }
        if rv < lv {
            return false;
        }
    }
    false
}
