use convert_case::{Case, Casing};

use crate::wrap_tf_type;

#[derive(Debug, Default)]
pub struct JsonnetRenderer {
    lines: Vec<String>,
}

impl JsonnetRenderer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add_line(&mut self, line: impl Into<String>) {
        self.lines.push(line.into())
    }

    pub fn add_doc_string(&mut self, name: &str, help: &str) {
        self.add_line(format!(
            "'#{name}':: {{ 'function': {{ help: |||\n{}\n||| }} }},",
            help.lines()
                .map(|line| { format!("  {line}") })
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    pub fn add_with_function(
        &mut self,
        name: &str,
        resource_type: &str,
        tf_name: &str,
        help: Option<&str>,
    ) -> String {
        let func_name = format!("with_{name}").to_case(Case::Camel);
        if let Some(help) = help {
            self.add_doc_string(&func_name, help);
        }
        self.add_line(format!("{func_name}(value):: self {{"));
        self.add_line(wrap_tf_type(name, resource_type, tf_name));
        self.add_line("},");
        self.render()
    }

    pub fn render(&self) -> String {
        self.lines.join("\n")
    }
}
