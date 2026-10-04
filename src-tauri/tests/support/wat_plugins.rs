// Tiny WebAssembly plugins written in WAT, shared by the unit tests under
// `src/plugins/external` and by the integration tests. They are the "hostile
// guests": each one misbehaves in exactly one way. The real SDK-built plugins
// are tested separately from `examples/plugins`.
//
// Memory map (2 pages = 128 KiB):
//   0..999      the reply JSON the body responds with
//   1000..1014  `{"status":"ok"}`, the default answer to `init`
//   2000..      event JSON      3000..  log text      4000..  spare
//   8192..      where `ld_alloc` puts the request (fixed, so nothing leaks)
//
// The request starts `{"op":"init"` or `{"op":"invoke","action":"X`, so a
// handler tells them apart by the byte at offset 9 (`i` or `v`) and branches on
// the first character of the action id at offset 25.

pub const INIT_OK: &str = r#"{"status":"ok"}"#;
pub const REPLY_AT: u32 = 0;
pub const EVENT_AT: u32 = 2000;
pub const LOG_AT: u32 = 3000;

fn esc(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                (b as char).to_string()
            } else {
                format!("\\{b:02x}")
            }
        })
        .collect()
}

pub struct Wat {
    abi: i32,
    init: String,
    body: String,
    data: Vec<(u32, String)>,
    funcs: String,
    imports: String,
    exports_memory: bool,
    exports_handle: bool,
}

impl Wat {
    pub fn new() -> Wat {
        Wat {
            abi: 1,
            init: format!(
                "(call $respond (i32.const 1000) (i32.const {})) (return (i32.const 0))",
                INIT_OK.len()
            ),
            body: "(i32.const 0)".into(),
            data: vec![(1000, INIT_OK.into())],
            funcs: String::new(),
            imports: String::new(),
            exports_memory: true,
            exports_handle: true,
        }
    }
    pub fn abi(mut self, n: i32) -> Wat {
        self.abi = n;
        self
    }
    /// WAT run when the request is `init`. Must `return` an i32.
    pub fn init(mut self, wat: &str) -> Wat {
        self.init = wat.into();
        self
    }
    /// WAT run for every other request. Must leave an i32 on the stack.
    pub fn body(mut self, wat: &str) -> Wat {
        self.body = wat.into();
        self
    }
    pub fn data(mut self, at: u32, s: &str) -> Wat {
        self.data.push((at, s.into()));
        self
    }
    pub fn funcs(mut self, wat: &str) -> Wat {
        self.funcs.push_str(wat);
        self
    }
    pub fn import(mut self, wat: &str) -> Wat {
        self.imports.push_str(wat);
        self
    }
    pub fn without_memory_export(mut self) -> Wat {
        self.exports_memory = false;
        self
    }
    pub fn without_handle(mut self) -> Wat {
        self.exports_handle = false;
        self
    }
    /// Respond with `json` to every invoke, and return 0.
    pub fn reply(self, json: &str) -> Wat {
        let n = json.len();
        self.data(REPLY_AT, json).body(&format!(
            "(call $respond (i32.const 0) (i32.const {n})) (i32.const 0)"
        ))
    }
    /// Publish `json` as an event (the return code is dropped).
    pub fn publish_code(json: &str) -> String {
        format!(
            "(drop (call $publish (i32.const {EVENT_AT}) (i32.const {})))",
            json.len()
        )
    }

    pub fn wat(&self) -> String {
        let data: String = self
            .data
            .iter()
            .map(|(at, s)| format!("(data (i32.const {at}) \"{}\")\n", esc(s)))
            .collect();
        let memory = if self.exports_memory {
            r#"(memory (export "memory") 2)"#
        } else {
            "(memory 2)"
        };
        let handle = if self.exports_handle {
            format!(
                r#"(func (export "ld_handle") (param $ptr i32) (param $len i32) (result i32)
                    (if (i32.eq (i32.load8_u (i32.add (local.get $ptr) (i32.const 9))) (i32.const 105))
                      (then {}))
                    {})"#,
                self.init, self.body
            )
        } else {
            String::new()
        };
        format!(
            r#"(module
              (import "launchdeck" "log" (func $log (param i32 i32 i32)))
              (import "launchdeck" "publish" (func $publish (param i32 i32) (result i32)))
              (import "launchdeck" "respond" (func $respond (param i32 i32)))
              {imports}
              {memory}
              (global $calls (mut i32) (i32.const 0))
              (func (export "ld_abi") (result i32) (i32.const {abi}))
              (func (export "ld_alloc") (param i32) (result i32) (i32.const 8192))
              {handle}
              {funcs}
              {data})"#,
            imports = self.imports,
            abi = self.abi,
            funcs = self.funcs,
        )
    }

    pub fn build(&self) -> Vec<u8> {
        wat::parse_str(self.wat()).unwrap_or_else(|e| panic!("bad WAT: {e}\n{}", self.wat()))
    }
}

/// Branch on the first character of the action id. Each arm must respond and
/// is followed by `return 0`; an unknown action returns code 7.
pub fn dispatch(arms: &[(char, &str)]) -> String {
    let mut out = String::new();
    for (c, body) in arms {
        out.push_str(&format!(
            "(if (i32.eq (i32.load8_u (i32.add (local.get $ptr) (i32.const 25))) (i32.const {})) (then {body} (return (i32.const 0))))\n",
            *c as u32
        ));
    }
    out.push_str("(i32.const 7)");
    out
}

/// WAT that responds with the JSON stored at `at`.
pub fn respond_at(at: u32, len: usize) -> String {
    format!("(call $respond (i32.const {at}) (i32.const {len}))")
}
