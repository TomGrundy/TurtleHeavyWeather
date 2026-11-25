use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{Position, PositionError, PositionOptions};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrowserCoords {
    pub lat: f64,
    pub lon: f64,
    pub accuracy_m: f64,
}

pub fn request_browser_location<F>(on_result: F)
where
    F: FnOnce(Result<BrowserCoords, String>) + 'static,
{
    let Some(window) = web_sys::window() else {
        on_result(Err("Browser window is unavailable.".into()));
        return;
    };
    let Ok(geo) = window.navigator().geolocation() else {
        on_result(Err("Geolocation is not supported by this browser.".into()));
        return;
    };

    let cb = Rc::new(RefCell::new(Some(on_result)));
    let cb_ok = Rc::clone(&cb);
    let cb_err = Rc::clone(&cb);

    let success = Closure::once(move |pos: Position| {
        let coords = pos.coords();
        let res = BrowserCoords {
            lat: coords.latitude(),
            lon: coords.longitude(),
            accuracy_m: coords.accuracy(),
        };
        if let Some(f) = cb_ok.borrow_mut().take() {
            f(Ok(res));
        }
    });

    let error = Closure::once(move |err: PositionError| {
        let msg = match err.code() {
            1 => "Location permission was denied. Please search by suburb or postcode instead."
                .into(),
            2 => "Location unavailable from browser/OS.".into(),
            3 => "Location request timed out.".into(),
            _ => format!("Geolocation error: {}", err.message()),
        };
        if let Some(f) = cb_err.borrow_mut().take() {
            f(Err(msg));
        }
    });

    let opts = PositionOptions::new();
    opts.set_enable_high_accuracy(false);
    opts.set_timeout(10_000);
    opts.set_maximum_age(300_000);

    if let Err(e) = geo.get_current_position_with_error_callback_and_options(
        success.as_ref().unchecked_ref(),
        Some(error.as_ref().unchecked_ref()),
        &opts,
    ) {
        if let Some(f) = cb.borrow_mut().take() {
            f(Err(format!("Failed to start geolocation: {e:?}")));
        }
        return;
    }

    success.forget();
    error.forget();
}
