// A UI screenshot tool for sandboxes with no browser (scripts/wkshot/shot.sh, app/docs/dev.md).
// Screenshot a page in WebKitGTK: wkshot <url> <out.png> <w>x<h> <wait ms> [js] [wait ms after js]
// The js runs as an async function body; whatever it sets as window.__r is printed before the shot.
use gtk::prelude::*;
use javascriptcore_rs::ValueExt;
use webkit2gtk::{SnapshotOptions, SnapshotRegion, WebViewExt};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (url, out) = (a[1].clone(), a[2].clone());
    let (w, h) = a[3].split_once('x').map(|(w, h)| (w.parse().unwrap(), h.parse().unwrap())).unwrap();
    let wait: u64 = a[4].parse().unwrap();
    let js = a.get(5).cloned().unwrap_or_default();
    let after: u64 = a.get(6).map(|s| s.parse().unwrap()).unwrap_or(800);
    gtk::init().unwrap();
    // broadway leaves the resolution at -1, which WebKit turns into a device scale of -1/96.
    gdk::Screen::default().unwrap().set_resolution(96.0);
    let win = gtk::Window::new(gtk::WindowType::Toplevel);
    win.set_default_size(w, h);
    let wv = webkit2gtk::WebView::new();
    wv.set_size_request(w, h);
    win.add(&wv);
    win.show_all();
    wv.load_uri(&url);
    let wv2 = wv.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(wait), move || {
        if !js.is_empty() {
            let body = format!("(async () => {{ {js} }})().then(v => window.__r = v, e => window.__r = 'error: ' + e); 0");
            wv2.evaluate_javascript(&body, None, None, None::<&gio::Cancellable>, |r| {
                if let Err(e) = r { eprintln!("js: {e}"); }
            });
        }
        let wv3 = wv2.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(after), move || {
            let wv4 = wv3.clone();
            wv3.evaluate_javascript("String(window.__r)", None, None, None::<&gio::Cancellable>, move |r| {
                if let Ok(v) = r { println!("js = {}", v.to_str()); }
                wv4.snapshot(SnapshotRegion::Visible, SnapshotOptions::NONE, None::<&gio::Cancellable>, move |r| {
                    match r {
                        Ok(s) => {
                            let s = cairo::ImageSurface::try_from(s).unwrap();
                            s.write_to_png(&mut std::fs::File::create(&out).unwrap()).unwrap();
                        }
                        Err(e) => eprintln!("snapshot: {e}"),
                    }
                    gtk::main_quit();
                });
            });
        });
    });
    gtk::main();
}
