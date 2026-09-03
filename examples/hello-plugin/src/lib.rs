//! The example plugin.
//!
//! Small on purpose. It exists to be the thing the host is tested against, so
//! it covers every shape in the WIT world and nothing else: a record with a
//! list in it (the theme), a plain list (the commands), a variant (the
//! outcome), and a host call that returns a string.

wit_bindgen::generate!({
    path: "../../crates/proton-plugin/wit",
    world: "plugin",
});

// The world exports bare functions rather than an interface, so bindgen puts
// the `Guest` trait and the types at the top level — the world's own
// `use editor.{...}` is what puts them there. `Swatch` is not in that list —
// it only appears inside a theme — so it keeps its interface path.
use proton::plugin::editor::Swatch;

struct Hello;

export!(Hello);

impl Guest for Hello {
    fn themes() -> Vec<Theme> {
        vec![Theme {
            id: "hello-dusk".into(),
            name: "Hello Dusk".into(),
            dark: true,
            colours: vec![
                Swatch {
                    name: "bg".into(),
                    value: "#161821".into(),
                },
                Swatch {
                    name: "ink".into(),
                    value: "#c6c8d1".into(),
                },
                Swatch {
                    name: "accent".into(),
                    value: "#84a0c6".into(),
                },
            ],
        }]
    }

    fn commands() -> Vec<Command> {
        vec![
            Command {
                id: "hello.greet".into(),
                title: "Hello: say hello".into(),
            },
            Command {
                id: "hello.count".into(),
                title: "Hello: count the lines in this buffer".into(),
            },
            Command {
                id: "hello.forever".into(),
                title: "Hello: never finish (on purpose)".into(),
            },
        ]
    }

    fn run(id: String) -> Outcome {
        match id.as_str() {
            "hello.greet" => Outcome::Insert("hello from a plugin\n".into()),

            // The one that proves a host call works: the plugin cannot see
            // the buffer, so it has to ask.
            "hello.count" => {
                let text = proton::plugin::editor::read_buffer();
                let (line, column) = proton::plugin::editor::cursor();
                Outcome::Message(format!(
                    "{} lines, cursor at {}:{}",
                    text.lines().count(),
                    line + 1,
                    column + 1
                ))
            }

            // Deliberately never returns. The editor has to survive this, and
            // a plugin that ships it on purpose is a better test than one
            // that hangs by accident.
            "hello.forever" => {
                let mut n: u64 = 0;
                loop {
                    n = n.wrapping_add(1);
                    std::hint::black_box(n);
                }
            }

            other => Outcome::Failed(format!("no such command: {other}")),
        }
    }
}
