//! TeX, live: TinyTeX, latexmk and texlab really downloaded and installed
//! into a folder of the test's own — never `~/.proton/tex` — and a first
//! page typeset in the sandbox. It downloads about 220 MB (78 MB with
//! `PROTON_TEX_BUNDLE=small`), so it is run by hand:
//!
//! ```text
//! cargo test -p proton-core --test live_tex -- --ignored --nocapture
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};

use proton_lab::tex::{platform, Bundle, TexEnv, TexState, Tools};

#[test]
#[ignore = "downloads TinyTeX and texlab from GitHub, about 220 MB"]
fn tinytex_latexmk_and_texlab_are_installed_and_typeset_a_first_page() {
    let person = std::env::temp_dir().join(format!("proton-live-tex-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&person);
    std::fs::create_dir_all(&person).unwrap();
    let home = person.join(".proton/tex");
    let mut tex = TexEnv::new(&home, Arc::new(proton_lab::github::Live::default()), platform(), Tools::find()).with_person(&person);
    tex.check();
    let bundle = if std::env::var("PROTON_TEX_BUNDLE").as_deref() == Ok("small") { Bundle::Small } else { Bundle::Full };
    tex.plan(bundle).unwrap();
    let wait = |tex: &mut TexEnv, limit: Duration| {
        let deadline = Instant::now() + limit;
        let mut last = String::new();
        while tex.busy() && Instant::now() < deadline {
            tex.poll();
            if let TexState::Making { stage, done, total, .. } = tex.state() {
                let now = format!("{stage} {}/{} MB", done / 1_000_000, total / 1_000_000);
                if now != last {
                    println!("  {now}");
                    last = now;
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    };
    wait(&mut tex, Duration::from_secs(60));
    let TexState::Planned { plan } = tex.state().clone() else { panic!("{:?}", tex.state()) };
    println!("plan: {} {} + texlab {}: {} MB to download", plan.bundle_name(), plan.tinytex, plan.texlab, plan.download / 1_000_000);
    tex.create().unwrap();
    wait(&mut tex, Duration::from_secs(30 * 60));
    assert!(matches!(tex.state(), TexState::Ready { .. }), "{:?}", tex.state());
    let marker = tex.marker().unwrap();
    println!("ready: {} MB, latexmk at {}", marker.size / 1_000_000, marker.latexmk().display());
    let _ = std::fs::remove_dir_all(&person);
}
