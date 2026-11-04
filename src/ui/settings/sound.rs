// SPDX-License-Identifier: GPL-2.0-or-later
//! Sound (audit-audio.md "Settings > Sound redesign"; docs/modules/audio.md): outputs with their
//! Bluetooth codec, the output slider with its 0 dB tick and "Allow louder", balance; inputs with a
//! slider capped at 0 dB, the live level, Automatic input level and Reduce background noise;
//! Bluetooth codec and headset autoswitch; Pro Audio rate, buffer and dropouts; Troubleshooting.
//!
//! Invariant: the page draws [`SoundState`] (the sound service's snapshot, frozen types from
//! `wol_core::audio`) and sends `wol_core::audio::Action`s back through a [`Source`]; it never asks
//! PipeWire itself. Outputs keep the shell's `System` path (default, volume, mute), as `wol audio`
//! and libwol do. The few user files the service does not own (keep awake, ignore hardware volume)
//! are WirePlumber fragments under the user's config, written journaled ([`awake_conf`],
//! [`soft_volume_conf`]).
use super::*;
use crate::copy::settings as copy;
use wol_core::audio::{
    ALLOWED_RATES, Action as AudioAction, Device, DeviceKind, InputMeter, ProAudio, QUANTA, db_of,
    hardware_percent, max_percent, percent_of, quantum_ms,
};

/// What the page draws: `wol_audio::Snapshot`'s shape, in the frozen `wol_core::audio` types.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SoundState {
    pub devices: Vec<Device>,
    pub meter: InputMeter,
    pub pro: ProAudio,
    pub auto_input_level: bool,
    pub noise_suppression: bool,
    /// "Allow louder": sliders may pass 0 dB.
    pub boost: bool,
    /// The sound service is connected (false while PipeWire restarts).
    pub connected: bool,
    /// The last action's problem, in plain words; None when it worked.
    pub error: Option<String>,
}

/// Settings the sound service does not report, read off the UI thread (`Machine::sound_extras`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Extras {
    /// -1.0 (left) ..= 1.0 (right), the default output's (`pactl get-sink-volume` "balance").
    pub balance: f32,
    /// `bluetooth.autoswitch-to-headset-profile` (WirePlumber setting; shipped on).
    pub autoswitch: bool,
    /// [`AWAKE_FILE`] exists.
    pub keep_awake: bool,
    /// [`SOFT_VOLUME_FILE`] exists.
    pub soft_volume: bool,
    /// Connected Bluetooth devices: (name, address), to find a codec's device.
    pub bluetooth: Vec<(String, String)>,
}

/// What the device lists show: (id, name, default, codec) per device ([`SoundState::shape`]).
type Shape = Vec<(u32, String, bool, Option<String>)>;

impl SoundState {
    /// The fixed laptop of golden renders: speakers, AirPods on AAC, a USB microphone with +20 dB
    /// of boost range at 0 dB, a 256-frame buffer.
    pub fn demo() -> Self {
        let dev = |id, name: &str, kind, default, volume, base, codec: Option<&str>| Device {
            id,
            name: name.into(),
            kind,
            default,
            volume,
            volume_base: base,
            muted: false,
            codec: codec.map(Into::into),
            profile: None,
        };
        SoundState {
            devices: vec![
                dev(
                    40,
                    "AirPods Pro",
                    DeviceKind::Output,
                    true,
                    0.216,
                    1.0,
                    Some("aac"),
                ),
                dev(
                    41,
                    "MacBook Speakers",
                    DeviceKind::Output,
                    false,
                    0.4,
                    1.0,
                    None,
                ),
                dev(
                    42,
                    "HDMI Display",
                    DeviceKind::Output,
                    false,
                    0.5,
                    1.0,
                    None,
                ),
                dev(50, "Yeti Nano", DeviceKind::Input, true, 0.1, 0.1, None),
                dev(
                    51,
                    "Built-in Microphone",
                    DeviceKind::Input,
                    false,
                    0.3,
                    1.0,
                    None,
                ),
            ],
            meter: InputMeter {
                device: 50,
                peak: 0.3,
                hold: 0.35,
                clipping: false,
            },
            pro: ProAudio {
                rate: 0,
                quantum: 256,
                xruns: 0,
            },
            auto_input_level: true,
            noise_suppression: false,
            boost: false,
            connected: true,
            error: None,
        }
    }
    fn default_of(&self, kind: DeviceKind) -> Option<&Device> {
        self.devices.iter().find(|d| d.kind == kind && d.default)
    }
    /// What the device lists show (ids, names, the defaults, codecs): a change rebuilds them;
    /// volumes and the meter only move the controls.
    fn shape(&self) -> Shape {
        self.devices
            .iter()
            .map(|d| (d.id, d.name.clone(), d.default, d.codec.clone()))
            .collect()
    }
}

impl Extras {
    /// The fixed laptop's extras (golden renders).
    pub fn demo() -> Self {
        Extras {
            balance: 0.0,
            autoswitch: true,
            keep_awake: false,
            soft_volume: false,
            bluetooth: vec![("AirPods Pro".into(), "aa:bb:cc:dd:ee:00".into())],
        }
    }
}

// ---- Pure rules (tested below) ----

/// Codecs the Bluetooth picker offers, best first (70-wolos-bluetooth.conf order): (id, label).
pub const BT_CODECS: [(&str, &str); 7] = [
    ("ldac", "LDAC"),
    ("aptx_hd", "aptX HD"),
    ("aptx", "aptX"),
    ("aac", "AAC"),
    ("opus_05", "Opus"),
    ("sbc_xq", "SBC XQ"),
    ("sbc", "SBC"),
];

/// The badge for a codec id (`api.bluez5.codec`): "AAC", "LDAC"; calls: "mSBC".
pub fn codec_label(codec: &str) -> String {
    match codec {
        "msbc" => "mSBC".into(),
        "cvsd" => "CVSD".into(),
        c => BT_CODECS
            .iter()
            .find(|(id, _)| *id == c)
            .map_or_else(|| c.to_uppercase(), |(_, l)| (*l).to_string()),
    }
}

/// What a device row says beside its name: the Bluetooth codec and the card profile that made
/// it, when that profile is not the plain one ("Pro Audio", "Calls"). QA-DEV-18: a Pro Audio
/// sink read like the speakers it replaced; QA-DEV-2: call mode was shown nowhere.
pub fn device_badge(d: &Device) -> String {
    let profile = match d.profile.as_deref() {
        Some("pro-audio") => Some("Pro Audio"),
        Some(p) if p.starts_with("headset-head-unit") => Some("Calls"),
        _ => None,
    };
    d.codec
        .as_deref()
        .map(codec_label)
        .into_iter()
        .chain(profile.map(String::from))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The icon for a device: Bluetooth headphones, a display, speakers or a microphone.
pub fn device_icon(d: &Device) -> &'static str {
    let name = d.name.to_lowercase();
    match d.kind {
        DeviceKind::Input => "audio-input-microphone-symbolic",
        _ if d.codec.is_some() => "audio-headphones-symbolic",
        _ if name.contains("hdmi") || name.contains("displayport") || name.contains("display") => {
            "video-display-symbolic"
        }
        _ if name.contains("headphone") || name.contains("headset") => "audio-headphones-symbolic",
        _ => "audio-speakers-symbolic",
    }
}

/// Sample rate picker: "Automatic" (0), then the allowed rates.
pub fn rate_options() -> Vec<(u32, String)> {
    let mut v = vec![(0, "Automatic".to_string())];
    v.extend(ALLOWED_RATES.iter().map(|r| {
        let k = f64::from(*r) / 1000.0;
        (*r, format!("{} kHz", trim_zero(k)))
    }));
    v
}

/// Buffer picker: "Automatic" (0), then each size with its delay at `rate` (48 kHz when automatic).
pub fn quantum_options(rate: u32) -> Vec<(u32, String)> {
    let rate = if rate == 0 { 48_000 } else { rate };
    let mut v = vec![(0, "Automatic".to_string())];
    v.extend(QUANTA.iter().map(|q| {
        (
            *q,
            format!(
                "{q} samples ({} ms)",
                trim_zero((f64::from(quantum_ms(*q, rate)) * 10.0).round() / 10.0)
            ),
        )
    }));
    v
}

/// "44.1", "48".
fn trim_zero(v: f64) -> String {
    let s = format!("{v:.1}");
    s.strip_suffix(".0").map_or(s.clone(), str::to_string)
}

/// "None", "1 since the sound service started", "12 since …".
pub fn dropouts_label(xruns: u32) -> String {
    match xruns {
        0 => "None".into(),
        n => format!("{n} since sound started"),
    }
}

/// The slider's end for `d`: 100 (0 dB), or the hardware maximum when louder is allowed.
pub fn slider_max(d: &Device, boost: bool) -> u16 {
    if boost {
        max_percent(d.volume_base)
    } else {
        100
    }
}

/// Meter fill 0..=1 for a linear peak: -60 dBFS empty, 0 dBFS full (a VU's useful range).
pub fn meter_fraction(peak: f32) -> f64 {
    if peak <= 0.0 {
        return 0.0;
    }
    f64::from(((db_of(peak, 1.0) + 60.0) / 60.0).clamp(0.0, 1.0))
}

/// `pactl get-sink-volume` output's balance line ("balance -0.20"); 0 when absent.
pub fn parse_balance(text: &str) -> f32 {
    text.lines()
        .find_map(|l| l.trim().strip_prefix("balance "))
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|v| v.is_finite())
        .map_or(0.0, |v| v.clamp(-1.0, 1.0))
}

/// Left and right percents for `volume` at `balance` (-1 left .. 1 right), as PulseAudio does it:
/// the far side is turned down, the near side keeps the volume.
pub fn balance_percents(volume: u16, balance: f32) -> (u16, u16) {
    let b = balance.clamp(-1.0, 1.0);
    let v = f32::from(volume);
    let side = |scale: f32| (v * scale).round().clamp(0.0, 1000.0) as u16;
    (side((1.0 - b).min(1.0)), side((1.0 + b).min(1.0)))
}

/// A slider position as `Action::Volume` takes it: whole percent, never above 100 (System's
/// `wpctl` path has no louder range; a value over 255 used to wrap to 255 here).
pub fn system_percent(v: u16) -> u8 {
    u8::try_from(v.min(100)).unwrap_or(100)
}

/// `pactl set-sink-volume @DEFAULT_SINK@ L% R%` (percents of the hardware maximum, as pactl counts:
/// [`hardware_percent`] converts a slider position).
pub fn balance_argv(volume: u16, balance: f32) -> Vec<String> {
    let (l, r) = balance_percents(volume, balance);
    argv(&[
        "pactl",
        "set-sink-volume",
        "@DEFAULT_SINK@",
        &format!("{l}%"),
        &format!("{r}%"),
    ])
}

/// `wpctl settings bluetooth.autoswitch-to-headset-profile` says false.
pub fn parse_autoswitch(text: &str) -> bool {
    !text.to_lowercase().contains("false")
}

/// `wpctl settings --save bluetooth.autoswitch-to-headset-profile <on>`: live and kept.
pub fn autoswitch_argv(on: bool) -> Vec<String> {
    argv(&[
        "wpctl",
        "settings",
        "--save",
        "bluetooth.autoswitch-to-headset-profile",
        if on { "true" } else { "false" },
    ])
}

/// User WirePlumber fragments, relative to `$XDG_CONFIG_HOME`.
pub const AWAKE_FILE: &str = "wireplumber/wireplumber.conf.d/72-wolos-keep-awake.conf";
pub const SOFT_VOLUME_FILE: &str = "wireplumber/wireplumber.conf.d/73-wolos-soft-volume.conf";

/// "Keep device awake": sound cards never suspend (72-wolos-alsa.conf ships 5 s).
pub fn awake_conf() -> String {
    "# Settings > Sound > Keep device awake (WolOS). Delete this file to undo.\n\
     monitor.alsa.rules = [\n  {\n    matches = [ { node.name = \"~alsa_output.*\" } { node.name = \"~alsa_input.*\" } ]\n    \
     actions = { update-props = { session.suspend-timeout-seconds = 0 } }\n  }\n]\n"
        .into()
}

/// "Ignore hardware volume": PipeWire scales the sound itself instead of the card's mixer.
pub fn soft_volume_conf() -> String {
    "# Settings > Sound > Ignore hardware volume (WolOS). Delete this file to undo.\n\
     monitor.alsa.rules = [\n  {\n    matches = [ { device.name = \"~alsa_card.*\" } ]\n    \
     actions = { update-props = { api.alsa.soft-mixer = true } }\n  }\n]\n"
        .into()
}

/// `wol audio doctor`: the report Copy diagnostics puts on the clipboard (15 s: it times pw-top).
const DOCTOR: [&str; 3] = ["wol", "audio", "doctor"];
const DOCTOR_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// The service's own line under the page: its last error, else "reconnecting" while it is down.
fn service_notice(s: &SoundState) -> Option<&str> {
    s.error
        .as_deref()
        .or_else(|| (!s.connected).then_some(copy::SOUND_RECONNECTING))
}

/// Whether a state shows an asked-for Reset audio: Some(false) when the service reports an
/// error, Some(true) when it is connected again with the input filters off (what a reset leaves),
/// None while it is still on its way.
fn reset_outcome(s: &SoundState) -> Option<bool> {
    if s.error.is_some() {
        return Some(false);
    }
    (s.connected && !s.auto_input_level && !s.noise_suppression).then_some(true)
}

// ---- Where the state comes from ----

/// What the page asks of the sound service.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Act(AudioAction),
    /// "Allow louder".
    Boost(bool),
    /// The page is on screen: run the input meter and the dropout count (false: stop them).
    Meter(bool),
}

/// The sound service as the page sees it.
pub trait Source {
    /// The newest state, when one arrived since the last call.
    fn latest(&self) -> Option<SoundState>;
    fn request(&self, r: Request);
    /// The service really moves the volumes (false for a fixed preview state): the output slider
    /// then goes to it ([`AudioAction::OutputVolume`]), else to `System`.
    fn live(&self) -> bool {
        false
    }
}

/// A fixed state (previews): the first call returns it; requests change nothing.
struct Fixed(RefCell<Option<SoundState>>);

impl Source for Fixed {
    fn latest(&self) -> Option<SoundState> {
        self.0.borrow_mut().take()
    }
    fn request(&self, _: Request) {}
}

/// The running system: the wol-audio service on its own threads (crates/audio).
struct Live(wol_audio::pw::Handle);

impl From<wol_audio::Snapshot> for SoundState {
    fn from(s: wol_audio::Snapshot) -> Self {
        SoundState {
            devices: s.devices,
            meter: s.meter,
            pro: s.pro,
            auto_input_level: s.auto_input_level,
            noise_suppression: s.noise_suppression,
            boost: s.boost,
            connected: s.connected,
            error: s.error,
        }
    }
}

impl Source for Live {
    fn latest(&self) -> Option<SoundState> {
        self.0.latest().map(SoundState::from)
    }
    fn live(&self) -> bool {
        true
    }
    fn request(&self, r: Request) {
        use wol_audio::service::Request as R;
        self.0.request(match r {
            Request::Act(a) => R::Act(a),
            Request::Boost(on) => R::Boost(on),
            Request::Meter(on) => R::Meter(on),
        });
    }
}

/// The page's source: the machine's fixed state when it has one (Demo), else the live service.
fn source(m: &Arc<dyn Machine>) -> Rc<dyn Source> {
    match m.sound() {
        Some(s) => Rc::new(Fixed(RefCell::new(Some(s)))),
        None => live(),
    }
}

/// Connects to the sound service (its own threads). Settings and the Control Center each start
/// one, the first time their Sound page opens; both send the same requests.
pub fn live() -> Rc<dyn Source> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    Rc::new(Live(wol_audio::pw::Handle::spawn(home)))
}

// ---- The page ----

/// The controls that move with each new state (lists rebuild only when their shape changes).
struct View {
    src: Rc<dyn Source>,
    ctx: Rc<Ctx>,
    /// What the user's last action here did (Mute, Copy, Reset, the switches): only actions write
    /// it, so the 100 ms state poll never erases it (audit5 S7-1).
    status: gtk::Label,
    /// The sound service's own word: its last error, or that it is reconnecting. Every state
    /// rewrites it.
    notice: gtk::Label,
    /// Reset audio was asked for: [`reset_outcome`] of the next states says when it is done.
    reset_pending: Cell<bool>,
    outputs: gtk::Box,
    inputs: gtk::Box,
    out_slider: gtk::Scale,
    out_touch: Rc<Touch>,
    out_mute: gtk::Switch,
    in_slider: gtk::Scale,
    in_touch: Rc<Touch>,
    in_mute: gtk::Switch,
    meter: gtk::ProgressBar,
    louder: gtk::Switch,
    auto_level: gtk::Switch,
    noise: gtk::Switch,
    /// Each switch's wait for the state that shows its flip (bug 8).
    echoes: Echoes,
    bluetooth: gtk::Box,
    pro: gtk::Box,
    /// Set while the page moves its own controls (their handlers then send nothing).
    quiet: Rc<Cell<bool>>,
    shape: RefCell<Shape>,
    state: RefCell<SoundState>,
    extras: RefCell<Extras>,
}

impl View {
    fn send(&self, r: Request) {
        if !self.quiet.get() {
            self.src.request(r)
        }
    }

    /// Draws `s`: lists when the devices changed, then every control's value.
    fn apply(self: &Rc<Self>, s: SoundState) {
        self.quiet.set(true);
        let shape = s.shape();
        if *self.shape.borrow() != shape {
            self.fill(&s);
            *self.shape.borrow_mut() = shape;
        }
        let out = s.default_of(DeviceKind::Output);
        let inp = s.default_of(DeviceKind::Input);
        // "Allow louder" is one setting for both sliders; a slider being moved is the user's, not
        // the poll's (A6-1, A6-3).
        let hold = Touch::hold(form::SETTLE_MS);
        set_scale(&self.out_slider, out, s.boost, self.out_touch.busy(hold));
        // A switch the user just flipped keeps their flip until a state shows it (bug 8).
        let e = &self.echoes;
        draw_switch(&self.out_mute, &e.out_mute, out.is_some_and(|d| d.muted));
        self.out_mute
            .set_sensitive(out.is_some() && !e.out_mute.waiting());
        set_scale(&self.in_slider, inp, s.boost, self.in_touch.busy(hold));
        draw_switch(&self.in_mute, &e.in_mute, inp.is_some_and(|d| d.muted));
        self.in_mute
            .set_sensitive(inp.is_some() && !e.in_mute.waiting());
        self.meter
            .set_fraction(meter_fraction(s.meter.hold.max(s.meter.peak)));
        draw_switch(&self.louder, &e.louder, s.boost);
        draw_switch(&self.auto_level, &e.auto_level, s.auto_input_level);
        draw_switch(&self.noise, &e.noise, s.noise_suppression);
        say(&self.notice, service_notice(&s).unwrap_or(""));
        if self.reset_pending.get()
            && let Some(r) = reset_outcome(&s)
        {
            self.reset_pending.set(false);
            // A failed reset's reason is the service's error, already on `notice`.
            say(&self.status, if r { copy::SOUND_RESET_DONE } else { "" });
        }
        let pro_changed = self.state.borrow().pro != s.pro;
        *self.state.borrow_mut() = s;
        if pro_changed || self.pro.first_child().is_none() {
            self.fill_pro();
        }
        self.quiet.set(false);
    }

    /// Rebuilds the output and input lists and the Bluetooth section.
    fn fill(self: &Rc<Self>, s: &SoundState) {
        for (list, kind) in [
            (&self.outputs, DeviceKind::Output),
            (&self.inputs, DeviceKind::Input),
        ] {
            clear(list);
            let devs: Vec<&Device> = s.devices.iter().filter(|d| d.kind == kind).collect();
            if devs.is_empty() {
                let none = match kind {
                    DeviceKind::Input => copy::SOUND_NO_INPUT,
                    _ => copy::SOUND_NO_OUTPUT,
                };
                list.append(&w::list_row("", none, "", None));
            }
            for d in devs {
                let badge = device_badge(d);
                let b = w::choice_row(device_icon(d), &d.name, &badge, d.default);
                let (me, id) = (Rc::downgrade(self), d.id);
                b.connect_clicked(move |_| {
                    if let Some(me) = me.upgrade() {
                        me.choose(kind, id)
                    }
                });
                list.append(&b);
            }
        }
        self.fill_bluetooth(s);
    }

    /// The default for `kind` becomes `id`: inputs through the service, outputs through System.
    fn choose(self: &Rc<Self>, kind: DeviceKind, id: u32) {
        match kind {
            DeviceKind::Input => self.send(Request::Act(AudioAction::Input(id))),
            DeviceKind::Output => {
                let mut s = self.state.borrow().clone();
                for d in s.devices.iter_mut().filter(|d| d.kind == kind) {
                    d.default = d.id == id;
                }
                let st = self.status.clone();
                self.ctx.act(
                    Action::Output(id.to_string()),
                    Rc::new(move |r| say(&st, r.err().map_or("", |e| e.message()))),
                );
                self.apply(s);
            }
        }
    }

    /// Codec picker and autoswitch, when a Bluetooth device plays.
    fn fill_bluetooth(self: &Rc<Self>, s: &SoundState) {
        clear(&self.bluetooth);
        let ex = self.extras.borrow().clone();
        let mut rows = Vec::new();
        for d in s
            .devices
            .iter()
            .filter(|d| d.kind == DeviceKind::Output && d.codec.is_some())
        {
            let Some(addr) = ex
                .bluetooth
                .iter()
                .find(|b| b.0 == d.name)
                .map(|b| b.1.clone())
            else {
                continue;
            };
            let codec = d.codec.clone().unwrap_or_default();
            let labels: Vec<&str> = BT_CODECS.iter().map(|c| c.1).collect();
            let at = BT_CODECS.iter().position(|c| c.0 == codec).unwrap_or(3);
            let src = self.src.clone();
            rows.push(form::picker_row(
                &format!("{} codec", d.name),
                "",
                &labels,
                at,
                move |i| {
                    if let Some(c) = BT_CODECS.get(i) {
                        src.request(Request::Act(AudioAction::BtCodec(
                            addr.clone(),
                            c.0.to_string(),
                        )))
                    }
                },
            ));
        }
        if !ex.bluetooth.is_empty() {
            let st = self.status.clone();
            rows.push(form::pending_toggle_row(
                "Use the headset microphone automatically",
                copy::SOUND_AUTOSWITCH,
                ex.autoswitch,
                move |on, done| {
                    let st = st.clone();
                    off_thread(
                        move || run_ok(&autoswitch_argv(on)),
                        move |ok| {
                            say(&st, if ok { "" } else { copy::SOUND_NOT_SAVED });
                            done(ok)
                        },
                    );
                },
            ));
        }
        if !rows.is_empty() {
            self.bluetooth.append(&w::section("Bluetooth", &rows));
        }
    }

    /// Sample rate, buffer and dropouts.
    fn fill_pro(self: &Rc<Self>) {
        clear(&self.pro);
        let pro = self.state.borrow().pro;
        let rates = rate_options();
        let quanta = quantum_options(pro.rate);
        let r_at = rates.iter().position(|r| r.0 == pro.rate).unwrap_or(0);
        let q_at = quanta.iter().position(|q| q.0 == pro.quantum).unwrap_or(0);
        let r_labels: Vec<&str> = rates.iter().map(|r| r.1.as_str()).collect();
        let q_labels: Vec<&str> = quanta.iter().map(|q| q.1.as_str()).collect();
        let (me, me2) = (Rc::downgrade(self), Rc::downgrade(self));
        let (rates2, quanta2) = (rates.clone(), quanta.clone());
        let rate = form::picker_row("Sample rate", "", &r_labels, r_at, move |i| {
            if let (Some(me), Some(r)) = (me.upgrade(), rates2.get(i)) {
                let q = me.state.borrow().pro.quantum;
                me.send(Request::Act(AudioAction::ProAudio {
                    rate: r.0,
                    quantum: q,
                }));
            }
        });
        let buffer = form::picker_row("Buffer size", "", &q_labels, q_at, move |i| {
            if let (Some(me), Some(q)) = (me2.upgrade(), quanta2.get(i)) {
                let r = me.state.borrow().pro.rate;
                me.send(Request::Act(AudioAction::ProAudio {
                    rate: r,
                    quantum: q.0,
                }));
            }
        });
        self.pro.append(&w::section(
            "Pro Audio",
            &[rate, buffer, info("Dropouts", &dropouts_label(pro.xruns))],
        ));
        self.pro.append(&w::footnote(copy::SOUND_PRO_NOTE));
    }
}

/// What a volume slider shows for a device ([`scale_plan`]).
#[derive(Clone, Debug, PartialEq)]
pub struct ScalePlan {
    /// The range ends here ([`slider_max`]).
    pub max: u16,
    /// A tick labelled "0 dB" at 100 (the slider can pass it).
    pub tick: bool,
    /// Where the knob goes; None while the user is moving it (the state is older than their hand).
    pub value: Option<u16>,
}

/// The slider for `d`: None when there is no device (the slider is off).
pub fn scale_plan(d: Option<&Device>, boost: bool, dragging: bool) -> Option<ScalePlan> {
    let d = d?;
    let max = slider_max(d, boost);
    Some(ScalePlan {
        max,
        tick: max > 100,
        value: (!dragging).then(|| percent_of(d.volume, d.volume_base, boost)),
    })
}

/// Draws [`scale_plan`] on `s`.
fn set_scale(s: &gtk::Scale, d: Option<&Device>, boost: bool, dragging: bool) {
    match scale_plan(d, boost, dragging) {
        Some(plan) => {
            s.set_sensitive(true);
            apply_scale_plan(s, &plan);
        }
        None => s.set_sensitive(false),
    }
}

/// Puts `plan` on a scale: the range and the 0 dB tick when they changed, the knob when it has
/// a value. Shared by Settings and the Control Center.
pub fn apply_scale_plan(s: &gtk::Scale, plan: &ScalePlan) {
    if (s.adjustment().upper() - f64::from(plan.max)).abs() > f64::EPSILON {
        s.set_range(0.0, f64::from(plan.max));
        s.clear_marks();
        if plan.tick {
            s.add_mark(100.0, gtk::PositionType::Bottom, Some("0 dB"));
        }
    }
    if let Some(v) = plan.value {
        s.set_value(f64::from(v));
    }
}

/// How long a Sound switch waits for the service to show the user's flip before it flips back.
const SWITCH_WAIT_MS: u64 = 2000;

/// A Sound switch the user flipped, until a state shows the flip (bug 8). States arrive every
/// 100 ms and lag the click, so drawing each one on the switch moved the knob back and forth
/// under fast clicks and left it half way. While it waits, [`View::apply`] leaves the switch
/// alone; the first state that reports `want` ends the wait (`done(true)`), and none within the
/// wait flips the switch back (`done(false)`).
#[derive(Default)]
struct Echo {
    want: Cell<Option<bool>>,
    done: RefCell<Option<Box<dyn FnOnce(bool)>>>,
    /// Counts waits, so a timer left from an ended one does nothing.
    round: Cell<u64>,
}

impl Echo {
    /// The flip to `want` was sent: wait up to `wait` for it, then `done(false)` and `gave_up()`.
    fn begin(
        self: &Rc<Self>,
        want: bool,
        done: Box<dyn FnOnce(bool)>,
        wait: std::time::Duration,
        gave_up: impl FnOnce() + 'static,
    ) {
        self.end(false);
        self.want.set(Some(want));
        *self.done.borrow_mut() = Some(done);
        let (me, round) = (Rc::downgrade(self), self.round.get());
        glib::timeout_add_local_once(wait, move || {
            if let Some(me) = me.upgrade()
                && me.round.get() == round
                && me.end(false)
            {
                gave_up()
            }
        });
    }

    fn waiting(&self) -> bool {
        self.want.get().is_some()
    }

    /// A state reports `now`: whether the page may draw it on the switch. A state that shows
    /// the flip ends the wait; an older one is not drawn.
    fn seen(&self, now: bool) -> bool {
        match self.want.get() {
            None => true,
            Some(w) if w == now => {
                self.end(true);
                true
            }
            Some(_) => false,
        }
    }

    /// Ends the wait with `done(ok)`; false when there was none.
    fn end(&self, ok: bool) -> bool {
        self.want.set(None);
        self.round.set(self.round.get() + 1);
        let done = self.done.borrow_mut().take();
        done.map(|d| d(ok)).is_some()
    }
}

/// One [`Echo`] per Sound switch.
#[derive(Default)]
struct Echoes {
    out_mute: Rc<Echo>,
    in_mute: Rc<Echo>,
    louder: Rc<Echo>,
    auto_level: Rc<Echo>,
    noise: Rc<Echo>,
}

/// Draws a state's `now` on a pending switch unless the user's flip is still on its way. State
/// first, so the switch takes it as the real state and does not start a change.
fn draw_switch(s: &gtk::Switch, echo: &Echo, now: bool) {
    if echo.seen(now) {
        s.set_state(now);
        s.set_active(now);
    }
}

/// A Sound switch on the pending pattern: a flip sends `ask(on)` and waits for its [`Echo`].
/// A fixed preview state never changes, so there the flip is taken at once.
fn sound_switch(
    echo: &Rc<Echo>,
    status: &gtk::Label,
    live: bool,
    ask: impl Fn(bool) + 'static,
) -> gtk::Switch {
    let (echo, st) = (echo.clone(), status.clone());
    w::pending_toggle(false, move |on, done| {
        if live {
            let st = st.clone();
            echo.begin(
                on,
                done,
                std::time::Duration::from_millis(SWITCH_WAIT_MS),
                move || say(&st, copy::SOUND_NOT_SAVED),
            );
            ask(on);
        } else {
            ask(on);
            done(true);
        }
    })
}

/// When the user last moved a slider. The page's state arrives every 100 ms (every buffer while
/// the meter runs) and is older than their hand until the service has carried out the change, so
/// a slider they touched lately is left alone (A6-3).
#[derive(Default)]
pub struct Touch(Cell<Option<std::time::Instant>>);

/// How long the service takes to show a change it was sent: the PipeWire round trip.
const ECHO_MS: u64 = 500;

impl Touch {
    pub fn moved(&self) {
        self.0.set(Some(std::time::Instant::now()));
    }
    /// A touch counts for `hold`: the slider's own delay before it sends plus [`ECHO_MS`].
    pub fn hold(send_delay_ms: u64) -> std::time::Duration {
        std::time::Duration::from_millis(send_delay_ms + ECHO_MS)
    }
    pub fn busy(&self, hold: std::time::Duration) -> bool {
        self.busy_at(std::time::Instant::now(), hold)
    }
    pub fn busy_at(&self, now: std::time::Instant, hold: std::time::Duration) -> bool {
        self.0
            .get()
            .is_some_and(|t| now.saturating_duration_since(t) < hold)
    }
}

/// A slider that reports once per drag ([`form::SETTLE_MS`] after the knob stops); moves made
/// while `quiet` (the page drawing a new state) report nothing.
fn volume_slider(
    quiet: &Rc<Cell<bool>>,
    touch: &Rc<Touch>,
    changed: impl Fn(u16) + 'static,
) -> gtk::Scale {
    let pending = Rc::new(Cell::new(None::<glib::SourceId>));
    let changed = Rc::new(changed);
    let (quiet, touch) = (quiet.clone(), touch.clone());
    let s = w::row_slider(0.0, move |v| {
        if quiet.get() {
            return;
        }
        touch.moved();
        if let Some(id) = pending.take() {
            id.remove();
        }
        let (p, f) = (pending.clone(), changed.clone());
        let id = glib::timeout_add_local_once(
            std::time::Duration::from_millis(form::SETTLE_MS),
            move || {
                p.set(None);
                f(v.round().clamp(0.0, 1000.0) as u16)
            },
        );
        pending.set(Some(id));
    });
    s.set_increments(1.0, 5.0);
    s
}

fn volume_line(slider: &gtk::Scale, low: &str, high: &str) -> gtk::Box {
    let b = w::hbox(w::S3);
    b.append(&w::icon(low, w::ICON));
    b.append(slider);
    b.append(&w::icon(high, w::ICON));
    b.set_hexpand(true);
    b
}

/// `argv` ran and exited 0 (blocks up to [`wol_core::system::RUN_TIMEOUT`]: off the GTK thread).
fn run_ok(argv: &[String]) -> bool {
    let a: Vec<&str> = argv.iter().map(String::as_str).collect();
    wol_core::system::run_with_timeout(&a, wol_core::system::RUN_TIMEOUT).is_some()
}

/// `systemctl --user restart wireplumber`: it reads the user fragments again.
const RESTART_WIREPLUMBER: [&str; 4] = ["systemctl", "--user", "restart", "wireplumber"];

/// How a fragment switch ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fragment {
    /// Saved, and WirePlumber restarted with it.
    Applied,
    /// Saved, but WirePlumber did not restart: it applies at the next login.
    SavedLater,
    /// Not saved: nothing changed.
    NotSaved,
}

/// Saves (`body`) or removes (None) the fragment `rel` under `config` (journaled), then runs
/// `restart` only when the save worked. Blocks: off the GTK thread.
fn fragment_change(
    config: &std::path::Path,
    rel: &str,
    body: Option<&str>,
    restart: impl FnOnce() -> bool,
) -> Fragment {
    let saved = wol_core::journal::Journal::for_config(config).save(
        "sound",
        "settings",
        &[(rel, body)],
        wol_core::journal::now(),
    );
    match saved {
        Err(_) => Fragment::NotSaved,
        Ok(_) if restart() => Fragment::Applied,
        Ok(_) => Fragment::SavedLater,
    }
}

/// Keep awake / Ignore hardware volume: save or remove the fragment (journaled) and restart
/// WirePlumber so it reads it, both off the GTK thread; the switch flips back when the save
/// failed (audit5 S7-2).
fn fragment_switch(
    rel: &'static str,
    text: fn() -> String,
    st: &gtk::Label,
) -> impl Fn(bool, Box<dyn FnOnce(bool)>) + use<> {
    let st = st.clone();
    move |on, done| {
        let st = st.clone();
        let config = PathBuf::from(kit::config_home());
        off_thread(
            move || {
                let body = on.then(text);
                fragment_change(&config, rel, body.as_deref(), || {
                    run_ok(&argv(&RESTART_WIREPLUMBER))
                })
            },
            move |r| {
                if r != Fragment::NotSaved {
                    changed_latest("sound");
                }
                say(
                    &st,
                    match r {
                        Fragment::Applied => "",
                        Fragment::SavedLater => copy::SOUND_SAVED_LATER,
                        Fragment::NotSaved => SAVE_FAILED,
                    },
                );
                done(r != Fragment::NotSaved)
            },
        );
    }
}

pub(super) fn page(e: &Rc<Env>) -> gtk::Box {
    let p = w::page("Sound");
    let status = status_label();
    let src = source(&e.machine());
    let quiet: Rc<Cell<bool>> = Rc::default();

    let outputs = w::vbox(0);
    let inputs = w::vbox(0);
    let view_cell: Rc<RefCell<Option<std::rc::Weak<View>>>> = Rc::default();
    let with_view = {
        let vc = view_cell.clone();
        move |f: &dyn Fn(&Rc<View>)| {
            if let Some(v) = vc.borrow().as_ref().and_then(std::rc::Weak::upgrade) {
                f(&v)
            }
        }
    };
    let with_view = Rc::new(with_view);

    // Output volume: the slider's scale is percent of 0 dB, so it goes to the sound service,
    // which knows each device's 0 dB; wpctl and pactl count from the hardware maximum (A6-2).
    // Only a fixed preview (no service) keeps System's path. Balance follows when not centred.
    let (out_touch, in_touch): (Rc<Touch>, Rc<Touch>) = Default::default();
    let out_slider = {
        let (wv, ctx) = (with_view.clone(), e.ctx.clone());
        volume_slider(&quiet, &out_touch, move |v| {
            wv(&|view| {
                let b = view.extras.borrow().balance;
                let base = view
                    .state
                    .borrow()
                    .default_of(DeviceKind::Output)
                    .map_or(1.0, |d| d.volume_base);
                if b.abs() > f32::EPSILON {
                    background(vec![balance_argv(hardware_percent(v, base), b)]);
                } else if view.src.live() {
                    view.send(Request::Act(AudioAction::OutputVolume(v)));
                } else {
                    ctx.act(Action::Volume(system_percent(v)), Rc::new(|_| {}));
                }
            })
        })
    };
    // The switches wait for the state that shows their flip ([`Echo`]); `quiet` is not needed
    // here: the page draws them with their state first, which starts no change.
    let echoes = Echoes::default();
    let live = src.live();
    let out_mute = {
        let (ctx, st, echo) = (e.ctx.clone(), status.clone(), echoes.out_mute.clone());
        sound_switch(&echoes.out_mute, &status, live, move |on| {
            // A refused mute flips back at once, under its own reason; a done one waits for the state.
            let echo = echo.clone();
            ctx.act_switch(
                Action::Muted(on),
                &st,
                Box::new(move |ok| {
                    if !ok {
                        echo.end(false);
                    }
                }),
            )
        })
    };
    let in_slider = {
        let wv = with_view.clone();
        volume_slider(&quiet, &in_touch, move |v| {
            wv(&|view| view.send(Request::Act(AudioAction::MicVolume(v))))
        })
    };
    let ask = |r: fn(bool) -> Request| {
        let src = src.clone();
        move |on| src.request(r(on))
    };
    let in_mute = sound_switch(
        &echoes.in_mute,
        &status,
        live,
        ask(|on| Request::Act(AudioAction::MicMuted(on))),
    );
    let louder = sound_switch(&echoes.louder, &status, live, ask(Request::Boost));
    let auto_level = sound_switch(
        &echoes.auto_level,
        &status,
        live,
        ask(|on| Request::Act(AudioAction::AutoInputLevel(on))),
    );
    let noise = sound_switch(
        &echoes.noise,
        &status,
        live,
        ask(|on| Request::Act(AudioAction::NoiseSuppression(on))),
    );
    let meter = w::progress();
    let bluetooth = w::vbox(w::S6);
    let pro = w::vbox(w::S2);

    p.append(&w::section(
        "",
        &[
            row(
                "Output volume",
                &volume_line(
                    &out_slider,
                    "audio-volume-low-symbolic",
                    "audio-volume-high-symbolic",
                ),
            ),
            row("Mute", &out_mute),
        ],
    ));
    // Balance: the default output's, from pactl; applied with the current volume.
    let balance_holder = w::vbox(0);
    let out_rows = w::vbox(0);
    out_rows.append(&outputs);
    out_rows.append(&balance_holder);
    out_rows.append(&row_sub("Allow louder", copy::SOUND_LOUDER, &louder));
    p.append(&w::section("Output", &[out_rows.upcast()]));
    let input_rows = w::vbox(0);
    input_rows.append(&inputs);
    input_rows.append(&row(
        "Input volume",
        &volume_line(
            &in_slider,
            "microphone-sensitivity-muted-symbolic",
            "microphone-sensitivity-high-symbolic",
        ),
    ));
    input_rows.append(&row("Input level", &meter));
    input_rows.append(&row("Mute input", &in_mute));
    input_rows.append(&row_sub(
        "Automatic input level",
        copy::SOUND_AUTO_LEVEL,
        &auto_level,
    ));
    input_rows.append(&row_sub(
        "Reduce background noise",
        copy::SOUND_NOISE,
        &noise,
    ));
    p.append(&w::section("Input", &[input_rows.upcast()]));
    p.append(&bluetooth);
    p.append(&pro);

    let troubleshooting = w::vbox(0);
    p.append(&troubleshooting);
    let notice = status_label();
    p.append(&notice);
    p.append(&status);

    let view = Rc::new(View {
        src: src.clone(),
        ctx: e.ctx.clone(),
        status: status.clone(),
        notice,
        reset_pending: Cell::new(false),
        outputs,
        inputs,
        out_slider,
        out_touch,
        out_mute,
        in_slider,
        in_touch,
        in_mute,
        meter,
        louder,
        auto_level,
        noise,
        echoes,
        bluetooth,
        pro,
        quiet,
        shape: RefCell::default(),
        state: RefCell::default(),
        extras: RefCell::default(),
    });
    *view_cell.borrow_mut() = Some(Rc::downgrade(&view));

    // Extras off the UI thread; then balance, Bluetooth and Troubleshooting.
    {
        let (m, v, st) = (e.machine(), view.clone(), status.clone());
        later(
            &troubleshooting,
            move || m.sound_extras(),
            move |into, ex| {
                *v.extras.borrow_mut() = ex.clone();
                let s = v.state.borrow().clone();
                v.fill_bluetooth(&s);
                balance_holder.append(&balance_row(&v, ex.balance));
                into.append(&troubleshooting_section(&v, &ex, &st));
            },
        );
    }

    // First state now (previews), then the newest every 100 ms while the page is on screen.
    if let Some(s) = src.latest() {
        view.apply(s);
    }
    let timer: Rc<Cell<Option<glib::SourceId>>> = Rc::default();
    {
        let (v, t) = (Rc::downgrade(&view), timer.clone());
        p.connect_map(move |_| {
            let Some(view) = v.upgrade() else { return };
            view.src.request(Request::Meter(true));
            let v2 = Rc::downgrade(&view);
            // 100 ms: a meter that moves smoothly to the eye, ten snapshots a second at most.
            let id = glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                let Some(view) = v2.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                if let Some(s) = view.src.latest() {
                    view.apply(s);
                }
                glib::ControlFlow::Continue
            });
            if let Some(old) = t.replace(Some(id)) {
                old.remove();
            }
        });
    }
    {
        let (v, t) = (Rc::downgrade(&view), timer);
        p.connect_unmap(move |_| {
            if let Some(id) = t.take() {
                id.remove();
            }
            if let Some(view) = v.upgrade() {
                view.src.request(Request::Meter(false));
            }
        });
    }
    // The view lives as long as the page.
    let keep = view.clone();
    p.connect_destroy(move |_| {
        let _ = &keep;
    });
    p
}

/// Balance slider: Left .. Right, centred = 0.
fn balance_row(v: &Rc<View>, balance: f32) -> gtk::Widget {
    let me = Rc::downgrade(v);
    form::slider_row(
        "Balance",
        (-1.0, 1.0, 0.1),
        f64::from(balance),
        ("Left", "Right"),
        move |b| {
            let Some(me) = me.upgrade() else { return };
            me.extras.borrow_mut().balance = b as f32;
            let s = me.state.borrow();
            let vol = s.default_of(DeviceKind::Output).map_or(0, |d| {
                hardware_percent(percent_of(d.volume, d.volume_base, s.boost), d.volume_base)
            });
            background(vec![balance_argv(vol, b as f32)]);
        },
    )
}

fn troubleshooting_section(v: &Rc<View>, ex: &Extras, st: &gtk::Label) -> gtk::Box {
    let awake = form::pending_toggle_row(
        "Keep device awake",
        copy::SOUND_AWAKE,
        ex.keep_awake,
        fragment_switch(AWAKE_FILE, awake_conf, st),
    );
    let soft = form::pending_toggle_row(
        "Ignore hardware volume",
        copy::SOUND_SOFT_VOLUME,
        ex.soft_volume,
        fragment_switch(SOFT_VOLUME_FILE, soft_volume_conf, st),
    );
    let diag = {
        let (st, anchor) = (st.clone(), v.status.clone());
        form::button_row("Diagnostics", "", "Copy", move || {
            let (st, anchor) = (st.clone(), anchor.clone());
            off_thread(
                || {
                    wol_core::system::run_with_timeout(&DOCTOR, DOCTOR_TIMEOUT)
                        .filter(|t| !t.trim().is_empty())
                },
                move |report| match report {
                    Some(t) => {
                        anchor.display().clipboard().set_text(&t);
                        say(&st, copy::SOUND_DIAG_COPIED);
                    }
                    None => say(&st, copy::SOUND_DIAG_NONE),
                },
            );
        })
    };
    let reset = {
        let (me, st) = (Rc::downgrade(v), st.clone());
        row_sub(
            "Reset audio",
            copy::SOUND_RESET_ABOUT,
            &w::button("Reset", w::Kind::Destructive, move || {
                if let Some(me) = me.upgrade() {
                    // The "done" sentence waits for a state that shows the reset (audit5 S7-1).
                    say(&st, "");
                    me.reset_pending.set(true);
                    me.send(Request::Act(AudioAction::ResetAudio));
                }
            }),
        )
    };
    w::section("Troubleshooting", &[awake, soft, diag, reset])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QA-DEV-18 / QA-DEV-2: the profile that made a device is named in its row.
    #[test]
    fn rows_name_pro_audio_and_call_mode() {
        let d = |codec: Option<&str>, profile: Option<&str>| Device {
            codec: codec.map(String::from),
            profile: profile.map(String::from),
            ..Device::default()
        };
        assert_eq!(device_badge(&d(None, Some("pro-audio"))), "Pro Audio");
        assert_eq!(
            device_badge(&d(Some("msbc"), Some("headset-head-unit"))),
            "mSBC · Calls"
        );
        assert_eq!(
            device_badge(&d(Some("aptx_hd"), Some("a2dp-sink"))),
            "aptX HD"
        );
        assert_eq!(device_badge(&d(None, Some("output:analog-stereo"))), "");
    }

    #[test]
    fn codecs_read_as_their_names() {
        assert_eq!(codec_label("aac"), "AAC");
        assert_eq!(codec_label("ldac"), "LDAC");
        assert_eq!(codec_label("aptx_hd"), "aptX HD");
        assert_eq!(codec_label("msbc"), "mSBC");
        assert_eq!(codec_label("lc3"), "LC3");
    }

    #[test]
    fn pro_audio_offers_the_graph_rates_and_buffers() {
        let r = rate_options();
        assert_eq!(r[0], (0, "Automatic".into()));
        assert_eq!(r[1], (44100, "44.1 kHz".into()));
        assert_eq!(r[2], (48000, "48 kHz".into()));
        assert_eq!(r.len(), ALLOWED_RATES.len() + 1);
        let q = quantum_options(0);
        assert_eq!(q[1], (64, "64 samples (1.3 ms)".into()));
        assert_eq!(q.len(), QUANTA.len() + 1);
        assert_eq!(quantum_options(96_000)[1].1, "64 samples (0.7 ms)");
        assert_eq!(dropouts_label(0), "None");
        assert_eq!(dropouts_label(3), "3 since sound started");
    }

    /// 100% is 0 dB: without "Allow louder" the slider stops there, with it the hardware maximum
    /// (+20 dB on a 0.1-base microphone, 215%) is reachable and the tick marks 100.
    #[test]
    fn sliders_stop_at_zero_db_unless_louder_is_allowed() {
        let mic = Device {
            kind: DeviceKind::Input,
            volume: 1.0,
            volume_base: 0.1,
            ..Device::default()
        };
        assert_eq!(slider_max(&mic, false), 100);
        assert_eq!(slider_max(&mic, true), max_percent(0.1));
        assert!(slider_max(&mic, true) > 200);
        let speakers = Device::default();
        assert_eq!(
            slider_max(&speakers, true),
            100,
            "no boost range: 100 is the top"
        );
    }

    /// A6-1: "Allow louder" reaches the input slider too; A6-3: a slider in the user's hand keeps
    /// its knob when a state arrives.
    #[test]
    fn the_scale_plan_follows_allow_louder_and_leaves_a_dragged_knob_alone() {
        let mic = Device {
            kind: DeviceKind::Input,
            volume: 0.1,
            volume_base: 0.1,
            ..Device::default()
        };
        assert_eq!(scale_plan(None, true, false), None, "no device, no slider");
        let quiet = scale_plan(Some(&mic), false, false).unwrap();
        assert_eq!(
            (quiet.max, quiet.tick, quiet.value),
            (100, false, Some(100))
        );
        let loud = scale_plan(Some(&mic), true, false).unwrap();
        assert_eq!(loud.max, max_percent(0.1));
        assert!(loud.max > 200 && loud.tick, "{loud:?}");
        let held = scale_plan(Some(&mic), true, true).unwrap();
        assert_eq!(
            held.value, None,
            "the poll does not move the knob under a finger"
        );
        assert_eq!(held.max, loud.max, "the range still follows the setting");
        // The input slider is fed the setting, like the output one (it was `false`).
        let src = include_str!("sound.rs");
        let apply = &src[src.find("fn apply(self: &Rc<Self>").unwrap()..];
        let apply = &apply[..apply.find("\n    }\n").unwrap()];
        assert!(
            apply.contains("set_scale(&self.in_slider, inp, s.boost,"),
            "{apply}"
        );
        assert!(
            apply.contains("set_scale(&self.out_slider, out, s.boost,"),
            "{apply}"
        );
    }

    #[test]
    fn a_touched_slider_is_the_users_until_the_service_has_answered() {
        use std::time::{Duration, Instant};
        let t = Touch::default();
        let hold = Touch::hold(form::SETTLE_MS);
        assert!(
            hold > Duration::from_millis(form::SETTLE_MS),
            "it outlasts the send delay"
        );
        assert!(!t.busy(hold), "never touched");
        t.moved();
        assert!(t.busy(hold));
        let now = Instant::now();
        assert!(t.busy_at(now + hold / 2, hold));
        assert!(!t.busy_at(now + hold + Duration::from_millis(50), hold));
        // Every drag step renews it, so a long drag is covered end to end.
        let src = include_str!("sound.rs");
        let slider = &src[src.find("fn volume_slider(").unwrap()..];
        let slider = &slider[..slider.find("\n}\n").unwrap()];
        assert!(slider.contains("touch.moved()"), "{slider}");
    }

    /// A6-2: the output slider's number is percent of 0 dB; pactl's is of the hardware maximum.
    #[test]
    fn balance_and_system_volume_are_converted_from_the_zero_db_scale() {
        assert_eq!(system_percent(0), 0);
        assert_eq!(system_percent(64), 64);
        assert_eq!(system_percent(100), 100);
        assert_eq!(
            system_percent(300),
            100,
            "no wrap to 255, no louder than System can"
        );
        assert_eq!(system_percent(u16::MAX), 100);
        // A DAC whose 0 dB is half way up: 100% on the slider is 50% for pactl.
        let hw = hardware_percent(100, 0.125);
        assert_eq!(hw, 50);
        assert_eq!(
            balance_argv(hw, 0.0),
            ["pactl", "set-sink-volume", "@DEFAULT_SINK@", "50%", "50%"]
        );
        // Nothing to convert on the usual device.
        assert_eq!(hardware_percent(80, 1.0), 80);
        // Only a fixed preview sends the output slider to System.
        assert!(!Fixed(RefCell::new(None)).live());
        let src = include_str!("sound.rs");
        assert!(
            src.contains("AudioAction::OutputVolume(v)"),
            "the live page sends the slider to the service"
        );
    }

    #[test]
    fn the_meter_spans_sixty_decibels() {
        assert_eq!(meter_fraction(0.0), 0.0);
        assert!((meter_fraction(1.0) - 1.0).abs() < 1e-6);
        assert!(
            (meter_fraction(0.001) - 0.0).abs() < 1e-6,
            "-60 dBFS is empty"
        );
        let half = meter_fraction(10f32.powf(-30.0 / 20.0));
        assert!((half - 0.5).abs() < 1e-3, "{half}");
    }

    #[test]
    fn balance_reads_and_turns_down_the_far_side() {
        let out = "Volume: front-left: 52428 /  80% / -5.81 dB,   front-right: 65536 / 100% / 0.00 dB\n        balance 0.20\n";
        assert!((parse_balance(out) - 0.2).abs() < 1e-6);
        assert_eq!(parse_balance("nothing"), 0.0);
        assert_eq!(parse_balance("balance 7"), 1.0);
        assert_eq!(balance_percents(80, 0.0), (80, 80));
        assert_eq!(balance_percents(80, 0.5), (40, 80));
        assert_eq!(balance_percents(80, -1.0), (80, 0));
        assert_eq!(
            balance_argv(50, 0.5),
            ["pactl", "set-sink-volume", "@DEFAULT_SINK@", "25%", "50%"]
        );
    }

    #[test]
    fn autoswitch_is_a_saved_wireplumber_setting() {
        assert!(parse_autoswitch("Value: true"));
        assert!(!parse_autoswitch("Value: false"));
        assert!(parse_autoswitch(""), "shipped on (71-wolos-input.conf)");
        assert_eq!(autoswitch_argv(false)[2], "--save");
        assert_eq!(autoswitch_argv(false)[4], "false");
    }

    /// The user fragments only touch ALSA nodes/cards and say how to undo them; WirePlumber
    /// appends rule arrays across fragments, so they add to 72-wolos-alsa.conf, not replace it.
    #[test]
    fn troubleshooting_fragments_are_alsa_rules() {
        for (f, t, key) in [
            (
                AWAKE_FILE,
                awake_conf(),
                "session.suspend-timeout-seconds = 0",
            ),
            (
                SOFT_VOLUME_FILE,
                soft_volume_conf(),
                "api.alsa.soft-mixer = true",
            ),
        ] {
            assert!(f.starts_with("wireplumber/wireplumber.conf.d/7"), "{f}");
            assert!(t.contains("monitor.alsa.rules = ["), "{t}");
            assert!(t.contains(key), "{t}");
            assert!(t.contains("~alsa_"), "{t}");
            assert!(!t.contains("override."), "{t}");
        }
    }

    /// The service line follows each state; action results live on their own label, so a state
    /// without an error no longer wipes them (audit5 S7-1).
    #[test]
    fn the_state_poll_only_writes_the_service_line() {
        let mut s = SoundState::demo();
        assert_eq!(service_notice(&s), None);
        s.connected = false;
        assert_eq!(service_notice(&s), Some(copy::SOUND_RECONNECTING));
        s.error = Some("Could not mute.".into());
        assert_eq!(service_notice(&s), Some("Could not mute."));
        // `apply` rewrites `notice`; it touches `status` only to settle a pending reset.
        let src = include_str!("sound.rs");
        let apply = &src[src.find("fn apply(self: &Rc<Self>").unwrap()..];
        let apply = &apply[..apply.find("\n    }\n").unwrap()];
        assert!(apply.contains("say(&self.notice"), "{apply}");
        assert_eq!(apply.matches("say(&self.status").count(), 1, "{apply}");
        assert!(apply.contains("reset_pending"), "{apply}");
    }

    /// Bug 8: a flipped switch keeps the flip through older states (they used to move the knob
    /// back mid-flip); the state that shows it ends the wait once; none within the wait flips it
    /// back and says so. `apply` draws all five switches through their wait.
    #[test]
    fn a_flipped_switch_waits_for_the_state_that_shows_it() {
        use std::time::{Duration, Instant};
        // The page's timers run on the default context (the GTK thread's); this test owns it.
        let main = glib::MainContext::default();
        // Other tests of this binary iterate the same context for a moment: wait for it.
        let until = Instant::now() + Duration::from_secs(10);
        let _owner = loop {
            match main.acquire() {
                Ok(owner) => break owner,
                Err(e) if Instant::now() > until => panic!("the default main context: {e}"),
                Err(_) => std::thread::yield_now(),
            }
        };
        {
            let echo = Rc::new(Echo::default());
            let ended: Rc<RefCell<Vec<bool>>> = Rc::default();
            let gave_up = Rc::new(Cell::new(0));
            let record = || {
                let e = ended.clone();
                Box::new(move |ok| e.borrow_mut().push(ok)) as Box<dyn FnOnce(bool)>
            };
            let count = || {
                let g = gave_up.clone();
                move || g.set(g.get() + 1)
            };
            assert!(echo.seen(false), "no flip: every state is drawn");
            echo.begin(true, record(), Duration::from_millis(30), count());
            assert!(echo.waiting());
            assert!(!echo.seen(false), "an older state is not drawn");
            assert!(ended.borrow().is_empty());
            assert!(echo.seen(true), "the state that shows the flip is");
            assert_eq!(*ended.borrow(), [true]);
            assert!(echo.seen(true) && echo.seen(false), "the wait is over");
            // The first wait's timer must not end anything once it fires.
            echo.begin(false, record(), Duration::from_millis(60), count());
            let until = Instant::now() + Duration::from_secs(5);
            while echo.waiting() && Instant::now() < until {
                main.iteration(false);
                std::thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(*ended.borrow(), [true, false], "flipped back once");
            assert_eq!(gave_up.get(), 1, "and said so once");
        }
        assert_eq!(SWITCH_WAIT_MS, 2000);
        let src = include_str!("sound.rs");
        let apply = &src[src.find("fn apply(self: &Rc<Self>").unwrap()..];
        let apply = &apply[..apply.find("\n    }\n").unwrap()];
        assert_eq!(apply.matches("draw_switch(&self.").count(), 5, "{apply}");
        assert!(!apply.contains(".set_active("), "{apply}");
    }

    /// "Sound settings were reset" waits for a state that shows the reset, not the click.
    #[test]
    fn reset_done_waits_for_the_reset_state() {
        let mut s = SoundState::demo();
        assert!(s.auto_input_level);
        assert_eq!(reset_outcome(&s), None, "filters still on: not reset yet");
        s.auto_input_level = false;
        s.connected = false;
        assert_eq!(reset_outcome(&s), None, "restarting");
        s.connected = true;
        assert_eq!(reset_outcome(&s), Some(true));
        s.error = Some("Sound could not be reset.".into());
        assert_eq!(reset_outcome(&s), Some(false));
    }

    /// A fragment that cannot be saved restarts nothing and the switch flips back; a restart that
    /// fails keeps the saved file for the next login (audit5 S7-2).
    #[test]
    fn fragment_switches_report_save_and_restart() {
        let dir = std::env::temp_dir().join(format!("wol-sound-frag-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // A file where the config folder should be: nothing can be written under it.
        let blocked = dir.join("not-a-folder");
        std::fs::write(&blocked, "").unwrap();
        let ran = Cell::new(false);
        let r = fragment_change(&blocked, AWAKE_FILE, Some(&awake_conf()), || {
            ran.set(true);
            true
        });
        assert_eq!(r, Fragment::NotSaved);
        assert!(!ran.get(), "no restart without a saved file");

        let home = dir.join("config");
        let body = awake_conf();
        assert_eq!(
            fragment_change(&home, AWAKE_FILE, Some(&body), || true),
            Fragment::Applied
        );
        assert_eq!(
            std::fs::read_to_string(home.join(AWAKE_FILE)).unwrap(),
            body
        );
        assert_eq!(
            fragment_change(&home, AWAKE_FILE, None, || false),
            Fragment::SavedLater
        );
        assert!(!home.join(AWAKE_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn devices_get_their_icons() {
        let mut d = Device {
            name: "AirPods Pro".into(),
            codec: Some("aac".into()),
            ..Device::default()
        };
        assert_eq!(device_icon(&d), "audio-headphones-symbolic");
        d.codec = None;
        d.name = "HDMI / DisplayPort 1".into();
        assert_eq!(device_icon(&d), "video-display-symbolic");
        d.name = "Speakers".into();
        assert_eq!(device_icon(&d), "audio-speakers-symbolic");
        d.kind = DeviceKind::Input;
        assert_eq!(device_icon(&d), "audio-input-microphone-symbolic");
    }

    /// The demo state draws every part of the page: a Bluetooth output with a codec that the
    /// extras can find, a boosted microphone at 0 dB, a forced buffer.
    #[test]
    fn the_demo_shows_every_section() {
        let s = SoundState::demo();
        let ex = Extras::demo();
        let bt = s
            .default_of(DeviceKind::Output)
            .and_then(|d| d.codec.clone());
        assert_eq!(bt.as_deref(), Some("aac"));
        assert!(ex.bluetooth.iter().any(|b| b.0 == "AirPods Pro"));
        let mic = s.default_of(DeviceKind::Input).cloned().unwrap_or_default();
        assert_eq!(percent_of(mic.volume, mic.volume_base, false), 100);
        assert!(QUANTA.contains(&s.pro.quantum));
    }
}
