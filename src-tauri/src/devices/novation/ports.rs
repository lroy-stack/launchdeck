//! Which MIDI ports belong to which Launchpad, on each system. Pure: a port
//! listing in, units out. Port names are hints only; the inquiry sent on
//! connect is what confirms the model.
//!
//! Each name is read on its own: the unit and direction markers the system
//! adds are taken off, the model is found from its tokens, and the role of
//! the interface from the token after the model's stem, a Windows wrapper or
//! the ALSA port number. MIDI-role ports of one model and one unit key are
//! then paired, inputs with outputs: by identical port id first (a duplex
//! ALSA port; the interface path of a Windows unit), then in list order (the
//! only clue macOS gives for identical units). DAW ports are recognised and
//! never paired. Every name format here is derived from docs and community
//! projects, NOT observed on any system (no Launchpad attached).

use super::profile::{ModelProfile, Role};
use crate::devices::midi::{PortInfo, PortSnapshot};
use crate::devices::source::UNSUPPORTED_MODEL;
use crate::devices::{DeviceIdentity, DeviceKind};
use crate::platform::Platform;

/// Tokens that start the name of a Novation model no profile describes.
const OTHER_NOVATION: [&str; 2] = ["launchpad", "lppromk3"];

/// Words that name an interface rather than a model: `MIDI`, `DAW`, `DIN`,
/// and their ALSA truncations.
const INTERFACE_WORDS: [&str; 5] = ["midi", "daw", "din", "da", "mi"];

/// A Launchpad of a known model, found on the ports: its MIDI-interface pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FoundUnit {
    pub profile: &'static ModelProfile,
    /// 1 for the model's first unit in the system's input order, then 2, …
    pub number: u32,
    /// What tells units apart on this system: the ALSA client on Linux, the
    /// `N- ` or ` (N)` marker on Windows; 1 without one.
    pub key: u32,
    /// `<model>:<number>`, e.g. `launchpad-x:1`.
    pub id: String,
    /// The model's name, with ` (2)` and so on from the second unit.
    pub name: String,
    pub input: PortInfo,
    pub output: PortInfo,
}

/// What one look at the ports found.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Parsed {
    /// Units of the given profiles, in the order of their MIDI input.
    pub units: Vec<FoundUnit>,
    /// Other Novation models, once per unit, never opened.
    pub unsupported: Vec<DeviceIdentity>,
}

/// What a port name says about its port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reading {
    pub model: Model,
    pub key: u32,
    /// The interface's role; `None` when the name does not tell.
    pub role: Option<Role>,
    /// The name without unit and direction markers, and the interface of a
    /// Windows wrapper (0 without one): the same for the same port of
    /// identical units.
    signature: (String, u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Model {
    Known(&'static ModelProfile),
    /// Another Novation model, by the name a person would read.
    Other(String),
}

/// Sorts the listed ports into units of the given models and other
/// Novation models.
pub(crate) fn parse(
    snapshot: &PortSnapshot,
    platform: Platform,
    profiles: &[&'static ModelProfile],
) -> Parsed {
    let read_all = |ports: &[PortInfo]| -> Vec<(PortInfo, Reading)> {
        ports
            .iter()
            .filter_map(|p| Some((p.clone(), read(&p.name, platform, profiles)?)))
            .collect()
    };
    let inputs = read_all(&snapshot.inputs);
    let outputs = read_all(&snapshot.outputs);
    Parsed {
        units: pair_units(&inputs, &outputs),
        unsupported: other_units(&inputs, &outputs),
    }
}

/// Reads one port name; `None` when it is not a Novation port.
pub(crate) fn read(
    name: &str,
    platform: Platform,
    profiles: &[&'static ModelProfile],
) -> Option<Reading> {
    let marked = Marked::strip(name, platform);
    let tokens = tokens(&marked.text);
    let known = profiles
        .iter()
        .find_map(|&profile| Some((profile, after_model(profile, &tokens)?)));
    let (model, role) = match known {
        Some((profile, after)) => (
            Model::Known(profile),
            role(profile, &marked, &tokens[after..], platform),
        ),
        None if tokens.iter().any(|t| OTHER_NOVATION.contains(&t.as_str())) => {
            (Model::Other(other_name(&marked)), None)
        }
        None => return None,
    };
    Some(Reading {
        model,
        key: marked.key,
        role,
        signature: (marked.text.to_lowercase(), marked.wrapper.unwrap_or(0)),
    })
}

/// The id of another model's unit: `unsupported:<slug of its name>`, and
/// `-2` and so on from the second unit.
pub(crate) fn unsupported(name: &str, number: u32) -> DeviceIdentity {
    let id = match number {
        1 => format!("unsupported:{}", slug(name)),
        n => format!("unsupported:{}-{n}", slug(name)),
    };
    DeviceIdentity {
        id,
        name: numbered(name, number),
        model: UNSUPPORTED_MODEL.into(),
        kind: DeviceKind::Hardware,
    }
}

/// A port name without what the system added to it.
struct Marked {
    text: String,
    key: u32,
    /// `n` of a Windows `MIDIIN<n> (…)` / `MIDIOUT<n> (…)` wrapper.
    wrapper: Option<u32>,
    /// The name ended in ` In` or ` Out`.
    direction: bool,
    /// Linux: the ALSA port number and the client's name.
    alsa_port: Option<u32>,
    client: Option<String>,
}

impl Marked {
    fn strip(name: &str, platform: Platform) -> Marked {
        let mut marked = Marked {
            text: name.trim().to_string(),
            key: 1,
            wrapper: None,
            direction: false,
            alsa_port: None,
            client: None,
        };
        match platform {
            Platform::Linux => marked.strip_alsa(),
            Platform::Windows => marked.strip_windows(),
            Platform::MacOs => {}
        }
        if let Some(rest) = strip_suffix_ignore_case(&marked.text, " in")
            .or_else(|| strip_suffix_ignore_case(&marked.text, " out"))
        {
            marked.text = rest.to_string();
            marked.direction = true;
        }
        marked
    }

    /// `client:port name <client>:<port>`: the client number is the unit key.
    fn strip_alsa(&mut self) {
        let Some((rest, client, port)) = alsa_address(&self.text) else {
            return;
        };
        self.key = client;
        self.alsa_port = Some(port);
        self.client = rest.split_once(':').map(|(c, _)| c.trim().to_string());
        self.text = rest.to_string();
    }

    /// A `MIDIIN<n> (…)` wrapper, a leading `N- ` (classic) and a ` (N)`
    /// (new style) unit marker.
    fn strip_windows(&mut self) {
        if let Some((n, inner)) = windows_wrapper(&self.text) {
            self.wrapper = Some(n);
            self.text = inner.to_string();
        }
        if let Some((n, rest)) = leading_marker(&self.text) {
            self.key = n;
            self.text = rest.to_string();
        }
        if let Some((n, rest)) = unit_suffix(&self.text) {
            self.key = n;
            self.text = rest;
        }
    }
}

fn alsa_address(name: &str) -> Option<(&str, u32, u32)> {
    let (rest, address) = name.rsplit_once(' ')?;
    let (client, port) = address.split_once(':')?;
    Some((rest, number(client)?, number(port)?))
}

fn windows_wrapper(name: &str) -> Option<(u32, &str)> {
    let rest = name
        .strip_prefix("MIDIIN")
        .or_else(|| name.strip_prefix("MIDIOUT"))?;
    let digits = rest.find(|c: char| !c.is_ascii_digit())?;
    let inner = rest[digits..].strip_prefix(" (")?.strip_suffix(')')?;
    Some((number(&rest[..digits])?, inner))
}

fn leading_marker(name: &str) -> Option<(u32, &str)> {
    let digits = name.find(|c: char| !c.is_ascii_digit())?;
    let rest = &name[digits..];
    let rest = rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix(" - "))?;
    Some((number(&name[..digits])?, rest))
}

/// ` (N)` at the end or before a space: `Launchpad X (2) LPX MIDI Out`.
fn unit_suffix(name: &str) -> Option<(u32, String)> {
    name.match_indices(" (").find_map(|(start, _)| {
        let rest = &name[start + 2..];
        let close = rest.find(')')?;
        let after = &rest[close + 1..];
        if !(after.is_empty() || after.starts_with(' ')) {
            return None;
        }
        Some((
            number(&rest[..close])?,
            format!("{}{after}", &name[..start]),
        ))
    })
}

/// A decimal number of digits only (no sign, not empty).
fn number(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn strip_suffix_ignore_case<'a>(text: &'a str, suffix: &str) -> Option<&'a str> {
    let start = text.len().checked_sub(suffix.len())?;
    let tail = text.get(start..)?;
    tail.eq_ignore_ascii_case(suffix).then(|| &text[..start])
}

fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Where the model's name ends among the tokens: after its last stem token
/// (`lpx`), or else after the last run of its name tokens (`launchpad x`,
/// so not `launchpad xl`).
fn after_model(profile: &ModelProfile, tokens: &[String]) -> Option<usize> {
    if let Some(i) = tokens
        .iter()
        .rposition(|t| profile.stem_tokens.contains(&t.as_str()))
    {
        return Some(i + 1);
    }
    let len = profile.name_tokens.len();
    (0..=tokens.len().checked_sub(len)?)
        .rev()
        .find(|&i| tokens[i..i + len].iter().eq(profile.name_tokens.iter()))
        .map(|i| i + len)
}

fn role(
    profile: &ModelProfile,
    marked: &Marked,
    after: &[String],
    platform: Platform,
) -> Option<Role> {
    if platform == Platform::Windows {
        match marked.wrapper {
            // The wrapper's number is the interface, whatever the inner name says.
            Some(n) if n >= 2 => return profile.interface(n),
            // A bare classic name is the first interface: `LPX MIDI` is the DAW one.
            None if !marked.direction => return profile.interface(1),
            _ => {}
        }
    }
    let word = after.first().map(String::as_str);
    let next = after.get(1).and_then(|t| number(t));
    match (word, next) {
        (Some("midi" | "mi"), Some(n)) => profile.interface(n),
        (Some("midi" | "mi"), None) => Some(Role::Midi),
        (Some("daw" | "da"), _) => Some(Role::Daw),
        _ if platform == Platform::Linux => profile.interface(marked.alsa_port? + 1),
        _ => None,
    }
}

/// The name of another model: the ALSA client, or the name without its
/// interface words and port stem (`Launchpad Pro MK3 LPProMK3 MIDI`).
fn other_name(marked: &Marked) -> String {
    if let Some(client) = marked.client.as_ref().filter(|c| !c.is_empty()) {
        return client.clone();
    }
    let mut words: Vec<&str> = marked.text.split_whitespace().collect();
    while words.len() > 1 && words.last().is_some_and(|w| is_interface_word(w)) {
        words.pop();
    }
    let ends_in_stem = words.last().is_some_and(|w| {
        let w = w.to_lowercase();
        w.starts_with("lp") && w != "launchpad"
    });
    if words.len() > 1 && ends_in_stem {
        words.pop();
    }
    words.join(" ")
}

fn is_interface_word(word: &str) -> bool {
    let w = word.to_lowercase();
    INTERFACE_WORDS.contains(&w.as_str()) || number(&w).is_some()
}

/// A MIDI-role port of a known model.
struct MidiPort<'a> {
    profile: &'static ModelProfile,
    key: u32,
    port: &'a PortInfo,
}

impl MidiPort<'_> {
    fn same_unit(&self, other: &MidiPort) -> bool {
        self.profile.model == other.profile.model && self.key == other.key
    }
}

fn midi_ports(list: &[(PortInfo, Reading)]) -> Vec<MidiPort<'_>> {
    list.iter()
        .filter_map(|(port, reading)| match (&reading.model, reading.role) {
            (Model::Known(profile), Some(Role::Midi)) => Some(MidiPort {
                profile,
                key: reading.key,
                port,
            }),
            _ => None,
        })
        .collect()
}

/// Units in the order of their MIDI input, numbered per model.
fn pair_units(inputs: &[(PortInfo, Reading)], outputs: &[(PortInfo, Reading)]) -> Vec<FoundUnit> {
    let ins = midi_ports(inputs);
    let outs = midi_ports(outputs);
    let mut counts = Vec::new();
    ins.iter()
        .zip(pair(&ins, &outs))
        .filter_map(|(input, o)| Some((input, &outs[o?])))
        .map(|(input, output)| {
            let number = next_number(&mut counts, input.profile.model);
            FoundUnit {
                profile: input.profile,
                number,
                key: input.key,
                id: format!("{}:{number}", input.profile.model),
                name: numbered(input.profile.name, number),
                input: input.port.clone(),
                output: output.port.clone(),
            }
        })
        .collect()
}

/// The output paired with each input: of the same model and unit key, the
/// one with the same id if there is one, else the first one left.
fn pair(ins: &[MidiPort], outs: &[MidiPort]) -> Vec<Option<usize>> {
    let mut taken = vec![false; outs.len()];
    let mut paired = vec![None; ins.len()];
    for by_id in [true, false] {
        for (i, input) in ins.iter().enumerate() {
            if paired[i].is_some() {
                continue;
            }
            let found = (0..outs.len()).find(|&o| {
                !taken[o]
                    && input.same_unit(&outs[o])
                    && (!by_id || input.port.id == outs[o].port.id)
            });
            if let Some(o) = found {
                taken[o] = true;
                paired[i] = Some(o);
            }
        }
    }
    paired
}

/// Other models, once per unit. A unit is a model name and a unit key; on
/// macOS identical units share both, so a port listed twice is two units.
fn other_units(
    inputs: &[(PortInfo, Reading)],
    outputs: &[(PortInfo, Reading)],
) -> Vec<DeviceIdentity> {
    // (name, key, units), in the order of first appearance.
    let mut groups: Vec<(String, u32, usize)> = Vec::new();
    for list in [inputs, outputs] {
        for (_, reading) in list {
            let Model::Other(name) = &reading.model else {
                continue;
            };
            let copies = list
                .iter()
                .filter(|(_, r)| r.key == reading.key && r.signature == reading.signature)
                .count();
            match groups
                .iter_mut()
                .find(|(n, k, _)| n == name && *k == reading.key)
            {
                Some(group) => group.2 = group.2.max(copies),
                None => groups.push((name.clone(), reading.key, copies)),
            }
        }
    }
    let mut counts = Vec::new();
    groups
        .iter()
        .flat_map(|(name, _, units)| std::iter::repeat_n(name, *units))
        .map(|name| unsupported(name, next_number(&mut counts, name)))
        .collect()
}

/// 1 for the first unit of a model, then 2, …
fn next_number<'a>(counts: &mut Vec<(&'a str, u32)>, model: &'a str) -> u32 {
    match counts.iter_mut().find(|(m, _)| *m == model) {
        Some((_, n)) => {
            *n += 1;
            *n
        }
        None => {
            counts.push((model, 1));
            1
        }
    }
}

fn numbered(name: &str, number: u32) -> String {
    match number {
        1 => name.to_string(),
        n => format!("{name} ({n})"),
    }
}

pub(crate) fn slug(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    slug.trim_matches('-').to_string()
}
