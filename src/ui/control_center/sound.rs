// SPDX-License-Identifier: GPL-2.0-or-later
//! Control Center Sound page (audit-audio): output and input pick, volume, mute and the Bluetooth
//! codec, drawn from the frozen `wol_core::audio::Device` model. The devices come from the
//! wol-audio service through Settings > Sound's seam ([`SoundSource`], [`from_state`]); while the
//! service has said nothing (no PipeWire) they come from `System` ([`snapshot`]). Outputs change
//! through `System` (`Action::Output/Muted`, as Settings does); input pick, level and mute go to
//! the service as `wol_core::audio::Action`s, and so does the output level whenever the service
//! is the one drawing the page: its slider counts from 0 dB, which `System`'s `wpctl` does not.
//! Invariant: 100% on a slider is 0 dB (`wol_core::audio::percent_of`), never the hardware maximum.
use super::{SOUND_SETTINGS, Ui, launch};
use crate::kit::{
    controls::{DetailPage, DeviceRow},
    widgets as w,
};
use crate::system::{Action, System};
use crate::ui::settings::{
    SoundRequest, SoundSource, SoundState, Touch, apply_scale_plan, scale_plan, system_percent,
};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use wol_core::audio::{self, Action as AudioAction, Device, DeviceKind};

/// The running sound service, through Settings > Sound's adapter (started once, on the first open).
pub(super) use crate::ui::settings::sound_live as live;

/// A fixed state (golden renders): handed out once; requests change nothing.
struct Fixed(RefCell<Option<SoundState>>);

impl SoundSource for Fixed {
    fn latest(&self) -> Option<SoundState> {
        self.0.borrow_mut().take()
    }
    fn request(&self, _: SoundRequest) {}
}

pub(super) fn fixed(s: SoundState) -> Rc<dyn SoundSource> {
    Rc::new(Fixed(RefCell::new(Some(s))))
}

/// The devices as the sound service reports them. An output's key is its node id, the value
/// Settings > Sound sends with `Action::Output` too.
pub(super) fn from_state(s: &SoundState) -> Snapshot {
    let of = |k: DeviceKind| -> Vec<Device> {
        s.devices.iter().filter(|d| d.kind == k).cloned().collect()
    };
    let outputs = of(DeviceKind::Output);
    Snapshot {
        keys: outputs.iter().map(|d| d.id.to_string()).collect(),
        inputs: of(DeviceKind::Input),
        outputs,
        boost: s.boost,
    }
}

/// What the Sound page draws.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Snapshot {
    pub outputs: Vec<Device>,
    pub inputs: Vec<Device>,
    /// What `Action::Output` takes for each output, in the order of `outputs`.
    pub keys: Vec<String>,
    /// "Allow louder": the sliders may pass 0 dB (the service's setting; `System` has none).
    pub boost: bool,
}

/// What decides a device row's look: everything but the level. A volume step changes `Device`
/// but not its row, and rebuilding the rows dropped the keyboard focus (QA-SET-50).
pub(super) type Shape = Vec<(u32, String, bool, Option<String>, DeviceKind)>;

impl Snapshot {
    /// The row-deciding part of the snapshot ([`Shape`]) and the keys the rows send.
    pub fn shape(&self) -> (Shape, Shape, &[String]) {
        let of = |l: &[Device]| -> Shape {
            l.iter()
                .map(|d| (d.id, d.name.clone(), d.default, d.codec.clone(), d.kind))
                .collect()
        };
        (of(&self.outputs), of(&self.inputs), &self.keys)
    }
    /// The default device of a kind (the first one when none is marked).
    pub fn default_of(&self, kind: DeviceKind) -> Option<&Device> {
        let list = match kind {
            DeviceKind::Output => &self.outputs,
            DeviceKind::Input => &self.inputs,
        };
        list.iter().find(|d| d.default).or(list.first())
    }
}

/// The devices as `System` reports them: every output (the default one carries the volume and
/// mute), and the default input when there is a microphone level.
pub(super) fn snapshot(s: &dyn System) -> Snapshot {
    let a = s.audio();
    let level = |v: Option<u8>| audio::linear_for(u16::from(v.unwrap_or(0)), 1.0);
    let outs = s.output_devices();
    let outputs = outs
        .iter()
        .enumerate()
        .map(|(i, o)| Device {
            id: u32::try_from(i).unwrap_or(u32::MAX),
            name: o.name.clone(),
            kind: DeviceKind::Output,
            default: o.default,
            volume: if o.default { level(a.volume) } else { 0.0 },
            muted: o.default && a.muted,
            ..Default::default()
        })
        .collect();
    let inputs = a
        .mic
        .map(|_| Device {
            id: 0,
            name: "Microphone".into(),
            kind: DeviceKind::Input,
            default: true,
            volume: level(a.mic),
            muted: a.mic_muted,
            ..Default::default()
        })
        .into_iter()
        .collect();
    Snapshot {
        outputs,
        inputs,
        keys: outs.into_iter().map(|o| o.id).collect(),
        boost: false,
    }
}

/// Slider position of a device: 100 is 0 dB.
pub(super) fn percent(d: &Device) -> u16 {
    audio::percent_of(d.volume, d.volume_base, false)
}

/// The codec as people know it (`ldac` -> "LDAC"); None for wired devices.
pub(super) fn codec_label(codec: Option<&str>) -> Option<String> {
    let c = codec?.trim();
    if c.is_empty() {
        return None;
    }
    Some(match c.to_ascii_lowercase().as_str() {
        "aptx" => "aptX".into(),
        "aptx_hd" => "aptX HD".into(),
        "aptx_ll" | "aptx_ll_duplex" => "aptX Low Latency".into(),
        "sbc_xq" => "SBC XQ".into(),
        "msbc" => "mSBC".into(),
        "faststream" | "faststream_duplex" => "FastStream".into(),
        o if o.starts_with("opus") => "Opus".into(),
        o => o.replace('_', " ").to_ascii_uppercase(),
    })
}

/// Row icon: microphone, headphones (by name or a Bluetooth codec) or speakers.
pub(super) fn icon(d: &Device) -> &'static str {
    let n = d.name.to_lowercase();
    if d.kind == DeviceKind::Input {
        "audio-input-microphone-symbolic"
    } else if d.codec.is_some()
        || ["headphone", "headset", "airpods", "buds"]
            .iter()
            .any(|k| n.contains(k))
    {
        "audio-headphones-symbolic"
    } else {
        "audio-speakers-symbolic"
    }
}

/// Volume icon of a device at its level.
fn level_icon(d: Option<&Device>) -> &'static str {
    let Some(d) = d else {
        return "audio-volume-muted-symbolic";
    };
    if d.kind == DeviceKind::Input {
        return if d.muted {
            "microphone-sensitivity-muted-symbolic"
        } else {
            "audio-input-microphone-symbolic"
        };
    }
    match percent(d) {
        p if d.muted || p == 0 => "audio-volume-muted-symbolic",
        1..34 => "audio-volume-low-symbolic",
        34..67 => "audio-volume-medium-symbolic",
        _ => "audio-volume-high-symbolic",
    }
}

/// Flat level control on a detail page: the fat slider (icon in the track) and a mute button.
struct Level {
    row: gtk::Box,
    scale: gtk::Scale,
    icon: gtk::Image,
    mute: gtk::Button,
    /// When the user last moved the slider: the page's own refresh leaves it alone meanwhile.
    touch: Rc<Touch>,
}

impl Level {
    /// `changed` gets the slider position (100 = 0 dB) for the user's moves, not for refreshes.
    fn new(ui: &Rc<Ui>, icon_name: &str, tip: &str, changed: impl Fn(u16) + 'static) -> Self {
        let row = w::hbox(w::S2);
        let o = gtk::Overlay::new();
        o.set_hexpand(true);
        let touch: Rc<Touch> = Default::default();
        let scale = {
            let (ui, touch) = (ui.clone(), touch.clone());
            w::slider(0.0, move |v| {
                if ui.syncing.get() {
                    return;
                }
                touch.moved();
                changed(v.round().clamp(0.0, 1000.0) as u16)
            })
        };
        scale.add_css_class("fat");
        o.set_child(Some(&scale));
        let icon = w::icon(icon_name, w::ICON);
        icon.set_halign(gtk::Align::Start);
        icon.add_css_class("fat-icon");
        icon.set_can_target(false);
        o.add_overlay(&icon);
        row.append(&o);
        let mute = w::icon_button("audio-volume-muted-symbolic", tip, || {});
        row.append(&mute);
        Level {
            row,
            scale,
            icon,
            mute,
            touch,
        }
    }
    /// Draws `d`: range from "Allow louder", knob unless `sliding` (the user has it).
    fn set(&self, d: Option<&Device>, boost: bool, sliding: bool) {
        self.row.set_visible(d.is_some());
        if let Some(plan) = scale_plan(d, boost, sliding) {
            apply_scale_plan(&self.scale, &plan);
        }
        let i = level_icon(d);
        if self.icon.icon_name().as_deref() != Some(i) {
            self.icon.set_icon_name(Some(i));
        }
    }
}

fn heading(text: &str) -> gtk::Label {
    let h = w::label(text, "secondary");
    h.add_css_class("detail-heading");
    h
}

type Src = Rc<RefCell<Option<Rc<dyn SoundSource>>>>;

pub(super) struct SoundPage {
    output: Level,
    input: Level,
    input_heading: gtk::Label,
    outputs: gtk::Box,
    inputs: gtk::Box,
    /// The sound service (None: `System` only).
    src: Src,
    /// Its newest state.
    state: RefCell<Option<SoundState>>,
    shown: Rc<RefCell<Option<Snapshot>>>,
    /// The page is drawn from the running service (not a fixed preview, not `System`): the
    /// output slider then goes to it.
    from_service: Rc<Cell<bool>>,
    /// Moves the output slider's level (100 = 0 dB): the service when it runs, else `System`.
    /// The main page's Sound slider uses it too, so both knobs mean the same level (QA-SET-49).
    pub set_out: Rc<dyn Fn(u16)>,
}

/// Sends `a` to the sound service, unless a refresh is moving the controls.
fn send(ui: &Ui, src: &Src, a: AudioAction) {
    if ui.syncing.get() {
        return;
    }
    if let Some(s) = src.borrow().as_ref() {
        s.request(SoundRequest::Act(a));
    }
}

/// The default device of `kind` in what the page shows.
fn shown_default(shown: &RefCell<Option<Snapshot>>, kind: DeviceKind) -> Option<Device> {
    shown.borrow().as_ref()?.default_of(kind).cloned()
}

impl SoundPage {
    pub fn build(ui: &Rc<Ui>) -> Self {
        let page = DetailPage::new("Sound", None, "", Some("Sound Settings…"));
        {
            let u = ui.clone();
            page.back.connect_clicked(move |_| u.show("main"));
        }
        if let Some(f) = &page.footer {
            f.connect_clicked(move |b| launch(b, SOUND_SETTINGS));
        }
        let src: Src = Default::default();
        let shown: Rc<RefCell<Option<Snapshot>>> = Default::default();
        let from_service: Rc<Cell<bool>> = Default::default();
        let set_out: Rc<dyn Fn(u16)> = {
            let (u, src, live) = (ui.clone(), src.clone(), from_service.clone());
            Rc::new(move |pct| {
                if live.get() {
                    // 100 = 0 dB, converted by the service (A6-2).
                    send(&u, &src, AudioAction::OutputVolume(pct));
                } else {
                    u.act_latest("volume", Action::Volume(system_percent(pct)));
                }
            })
        };
        let output = {
            let set = set_out.clone();
            Level::new(ui, "audio-volume-high-symbolic", "Mute", move |pct| {
                set(pct)
            })
        };
        {
            let (u, shown) = (ui.clone(), shown.clone());
            output.mute.connect_clicked(move |_| {
                let muted = shown_default(&shown, DeviceKind::Output).is_some_and(|d| d.muted);
                u.act(Action::Muted(!muted));
            });
        }
        let input = {
            let (u, src) = (ui.clone(), src.clone());
            Level::new(
                ui,
                "audio-input-microphone-symbolic",
                "Mute Microphone",
                move |pct| send(&u, &src, AudioAction::MicVolume(pct)),
            )
        };
        {
            let (u, src, shown) = (ui.clone(), src.clone(), shown.clone());
            input.mute.connect_clicked(move |_| {
                let muted = shown_default(&shown, DeviceKind::Input).is_some_and(|d| d.muted);
                send(&u, &src, AudioAction::MicMuted(!muted));
            });
        }
        let input_heading = heading("Input");
        let (outputs, inputs) = (w::vbox(0), w::vbox(0));
        page.set_rows(
            &[
                heading("Output").upcast(),
                output.row.clone().upcast(),
                outputs.clone().upcast(),
                input_heading.clone().upcast(),
                input.row.clone().upcast(),
                inputs.clone().upcast(),
            ],
            "",
        );
        ui.pages.add_named(&page.root, Some("sound"));
        SoundPage {
            output,
            input,
            input_heading,
            outputs,
            inputs,
            src,
            state: Default::default(),
            shown,
            from_service,
            set_out,
        }
    }

    /// Lets go of the sound service (the panel closed): its client leaves PipeWire, and the
    /// next show connects again (QA-SET-48).
    pub fn clear_source(&self) {
        *self.src.borrow_mut() = None;
        *self.state.borrow_mut() = None;
        self.from_service.set(false);
    }

    /// The default output's slider position (100 = 0 dB) while the service answers, else None.
    pub fn service_level(&self, sys: &dyn System) -> Option<u16> {
        self.poll();
        if !self.service_live() {
            return None;
        }
        self.current(sys)
            .default_of(DeviceKind::Output)
            .map(|d| percent(d).min(100))
    }

    /// The page is drawn from the running service with devices (not a preview, not `System`).
    fn service_live(&self) -> bool {
        self.src.borrow().as_ref().is_some_and(|s| s.live())
            && self
                .state
                .borrow()
                .as_ref()
                .is_some_and(|s| !s.devices.is_empty())
    }

    /// Where the page reads and sends sound from now on.
    pub fn set_source(&self, s: Rc<dyn SoundSource>) {
        *self.src.borrow_mut() = Some(s);
    }

    pub fn has_source(&self) -> bool {
        self.src.borrow().is_some()
    }

    /// Takes the service's newest state; true when one arrived.
    pub fn poll(&self) -> bool {
        let next = self.src.borrow().as_ref().and_then(|s| s.latest());
        let new = next.is_some();
        if new {
            *self.state.borrow_mut() = next;
        }
        new
    }

    /// What to draw: the service's devices, else `System`'s.
    fn current(&self, sys: &dyn System) -> Snapshot {
        match self.state.borrow().as_ref() {
            Some(s) if !s.devices.is_empty() => from_state(s),
            _ => snapshot(sys),
        }
    }

    /// Called from `ControlCenter::sync` (handlers muted).
    pub fn sync(&self, ui: &Rc<Ui>) {
        self.poll();
        let snap = self.current(&*ui.sys);
        // Input changes need the service; without it the input half only shows the level.
        let live = self.has_source() && self.state.borrow().is_some();
        self.input.scale.set_sensitive(live);
        self.input.mute.set_sensitive(live);
        self.from_service.set(self.service_live());
        let hold = Touch::hold(0);
        self.output.set(
            snap.default_of(DeviceKind::Output),
            snap.boost,
            ui.sliding("volume") || self.output.touch.busy(hold),
        );
        self.input.set(
            snap.default_of(DeviceKind::Input),
            snap.boost,
            self.input.touch.busy(hold),
        );
        self.input_heading.set_visible(!snap.inputs.is_empty());
        // Same rows (only a level moved): keep the widgets, and with them the focus.
        let same_rows = self
            .shown
            .borrow()
            .as_ref()
            .is_some_and(|s| s.shape() == snap.shape());
        if same_rows {
            *self.shown.borrow_mut() = Some(snap);
            return;
        }
        fill(&self.outputs, &snap.outputs, "No output devices", |i| {
            let (u, key) = (ui.clone(), snap.keys.get(i).cloned());
            move || {
                if let Some(k) = &key {
                    u.act(Action::Output(k.clone()))
                }
            }
        });
        fill(&self.inputs, &snap.inputs, "", |i| {
            let (u, src) = (ui.clone(), self.src.clone());
            let id = snap.inputs.get(i).map(|d| d.id);
            move || {
                if let Some(id) = id {
                    send(&u, &src, AudioAction::Input(id))
                }
            }
        });
        *self.shown.borrow_mut() = Some(snap);
    }
}

/// Replaces the rows of `list` with one per device (check mark on the default one); `empty`
/// shows as a label when there are none (nothing when it is empty).
fn fill<F: Fn() + 'static>(
    list: &gtk::Box,
    devices: &[Device],
    empty: &str,
    pick: impl Fn(usize) -> F,
) {
    while let Some(c) = list.first_child() {
        list.remove(&c);
    }
    for (i, d) in devices.iter().enumerate() {
        let sub = codec_label(d.codec.as_deref()).unwrap_or_default();
        let r = DeviceRow::new(
            icon(d),
            &d.name,
            &sub,
            d.default,
            d.default.then_some("object-select-symbolic"),
        );
        let f = pick(i);
        r.button.connect_clicked(move |_| f());
        list.append(&r.button);
    }
    if devices.is_empty() && !empty.is_empty() {
        let l = w::label(empty, "secondary");
        l.add_css_class("detail-empty");
        list.append(&l);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QA-SET-50: a volume step changes the snapshot, not the rows.
    #[test]
    fn a_level_change_keeps_the_rows() {
        let d = |volume| Device {
            id: 3,
            name: "Speakers".into(),
            default: true,
            volume,
            ..Default::default()
        };
        let snap = |volume| Snapshot {
            outputs: vec![d(volume)],
            keys: vec!["3".into()],
            ..Default::default()
        };
        assert_ne!(snap(0.2), snap(0.4));
        assert_eq!(snap(0.2).shape(), snap(0.4).shape());
        let mut other = snap(0.2);
        other.outputs[0].default = false;
        assert_ne!(snap(0.2).shape(), other.shape());
    }

    #[test]
    fn codecs_read_as_people_know_them() {
        for (c, want) in [
            (None, None),
            (Some(""), None),
            (Some("ldac"), Some("LDAC")),
            (Some("aac"), Some("AAC")),
            (Some("aptx_hd"), Some("aptX HD")),
            (Some("sbc_xq"), Some("SBC XQ")),
            (Some("opus_05_duplex"), Some("Opus")),
            (Some("lc3"), Some("LC3")),
        ] {
            assert_eq!(codec_label(c).as_deref(), want, "{c:?}");
        }
    }

    #[test]
    fn a_bluetooth_codec_or_name_means_headphones() {
        let d = |name: &str, codec: Option<&str>, kind| Device {
            name: name.into(),
            codec: codec.map(String::from),
            kind,
            ..Default::default()
        };
        assert_eq!(
            icon(&d("Built-in Speakers", None, DeviceKind::Output)),
            "audio-speakers-symbolic"
        );
        assert_eq!(
            icon(&d("Headphones", None, DeviceKind::Output)),
            "audio-headphones-symbolic"
        );
        assert_eq!(
            icon(&d("Sony WH", Some("ldac"), DeviceKind::Output)),
            "audio-headphones-symbolic"
        );
        assert_eq!(
            icon(&d("Headset Mic", None, DeviceKind::Input)),
            "audio-input-microphone-symbolic"
        );
    }

    /// The service's devices split by kind; an output's key is its node id.
    #[test]
    fn the_service_state_maps_by_kind() {
        let s = from_state(&SoundState::demo());
        assert!(!s.outputs.is_empty() && !s.inputs.is_empty());
        assert!(s.outputs.iter().all(|d| d.kind == DeviceKind::Output));
        assert!(s.inputs.iter().all(|d| d.kind == DeviceKind::Input));
        let keys: Vec<String> = s.outputs.iter().map(|d| d.id.to_string()).collect();
        assert_eq!(s.keys, keys);
        assert!(
            s.outputs
                .iter()
                .any(|d| codec_label(d.codec.as_deref()).is_some())
        );
        assert_eq!(from_state(&SoundState::default()), Snapshot::default());
    }

    /// A6-1: "Allow louder" is the service's setting; the page's sliders get it with the devices,
    /// and `System`'s snapshot has none.
    #[test]
    fn the_snapshot_carries_allow_louder() {
        let mut st = SoundState::demo();
        assert!(!from_state(&st).boost);
        st.boost = true;
        let snap = from_state(&st);
        assert!(snap.boost);
        let mic = snap.default_of(DeviceKind::Input).unwrap();
        let plan = scale_plan(Some(mic), snap.boost, false).unwrap();
        assert!(
            plan.max > 100 && plan.tick,
            "the demo microphone has +20 dB to give: {plan:?}"
        );
        assert_eq!(scale_plan(Some(mic), false, false).unwrap().max, 100);
        assert!(!snapshot(&crate::system::Fake).boost);
    }

    /// The default output carries System's volume and mute; 100% on the slider is 0 dB.
    #[test]
    fn the_snapshot_follows_system() {
        let s = snapshot(&crate::system::Fake);
        assert_eq!(s.outputs.len(), s.keys.len());
        let out = s.default_of(DeviceKind::Output).unwrap();
        assert_eq!(out.name, "Built-in Speakers");
        assert_eq!(percent(out), 64);
        assert!(!out.muted);
        assert_eq!(s.keys[0], "speakers");
        let mic = s.default_of(DeviceKind::Input).unwrap();
        assert_eq!(percent(mic), 50);
        let quiet = snapshot(&crate::system::FakeDesktop);
        assert!(quiet.inputs.iter().all(|d| d.kind == DeviceKind::Input));
        assert_eq!(level_icon(None), "audio-volume-muted-symbolic");
    }
}
