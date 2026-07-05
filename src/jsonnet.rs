use std::{
    fmt::{self, Display},
    ops,
};

use convert_case::{Case, Casing};
use derive_more::Display;

use crate::wrap_tf_type;

#[derive(Debug, Default)]
pub struct JsonnetRenderer {
    lines: Vec<String>,
    root: Child,
}
#[derive(Debug, Default, Display)]
pub enum Child {
    Object(Object),
    #[display("'{_0}'")]
    String(String),
    Code(String),
    Binary(Box<Binary>),

    #[default]
    #[display("nothing")]
    Empty,
}

impl ops::Add<Child> for Child {
    type Output = Child;
    fn add(self, rhs: Child) -> Self::Output {
        Child::Binary(Box::new(Binary {
            op: "+".into(),
            first: self,
            second: rhs,
        }))
    }
}

#[derive(Debug, Default)]
pub struct Binary {
    pub first: Child,
    pub second: Child,
    pub op: String,
}

impl Display for Binary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.first, self.op, self.second)
    }
}

#[derive(Debug, Default)]
pub struct Local {
    pub name: String,
    pub body: Child,
}

impl Display for Local {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "local {} = {}", self.name, self.body)
    }
}

#[derive(Debug, Default)]
pub struct Object {
    pub locals: Vec<Local>,
    pub asserts: Vec<String>,
    pub fields: Vec<ObjectEntry>,

    /// Additional lines
    pub lines: Vec<String>,
}

#[derive(Debug, Default)]
pub struct ObjectEntry {
    pub field: ObjectField,
    pub hidden: bool,
    pub body: Child,
}

impl Display for ObjectEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}{},",
            self.field,
            if self.hidden { "::" } else { ":" },
            self.body
        )
    }
}

#[derive(Debug, Default)]
pub struct Function {
    pub name: String,
    pub args: Vec<FunctionArg>,
}

impl Display for Function {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}({})",
            self.name,
            self.args
                .iter()
                .map(|arg| arg.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

#[derive(Debug, Default)]
pub struct FunctionArg {
    pub name: String,
    pub default: Option<Child>,
}

impl Display for FunctionArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}",
            self.name,
            if let Some(child) = &self.default {
                child.to_string()
            } else {
                "".to_string()
            }
        )
    }
}

#[derive(Debug, Default, Display)]
pub enum ObjectField {
    Plain(String),
    Function(Function),

    #[default]
    Invalid,
}

impl Display for Object {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{{")?;
        for local in &self.locals {
            writeln!(f, "{local},")?;
        }
        for field in &self.fields {
            writeln!(f, "{field}")?;
        }

        writeln!(f, "{}", self.lines.join("\n"))?;
        writeln!(f, "}}")?;

        Ok(())
    }
}

impl Object {
    pub fn new() -> Self {
        Self {
            ..Default::default()
        }
    }
    pub fn add_line(&mut self, line: impl Into<String>) {
        self.lines.push(line.into())
    }

    pub fn add_code_field(&mut self, name: impl Into<String>, val: impl Into<String>) {
        self.add_child_field(name, Child::Code(val.into()));
    }

    pub fn add_string_field(&mut self, name: impl Into<String>, val: impl Into<String>) {
        self.add_child_field(name, Child::String(val.into()));
    }
    pub fn add_child_field(&mut self, name: impl Into<String>, val: Child) {
        self.fields.push(ObjectEntry {
            field: ObjectField::Plain(name.into()),
            body: val,
            hidden: true,
        });
    }

    pub fn add_doc_string(&mut self, name: &str, help: &str) {
        self.add_code_field(
            format!("'#{name}'"),
            format!(
                "{{ 'function': {{ help: |||\n{}\n|||\n }} }}",
                help.lines()
                    .map(|line| { format!("  {line}") })
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        );
    }

    pub fn add_with_function(
        &mut self,
        name: &str,
        resource_type: &str,
        tf_name: &str,
        help: Option<&str>,
    ) {
        let func_name = format!("with_{name}").to_case(Case::Camel);
        if let Some(help) = help {
            self.add_doc_string(&func_name, help);
        }
        self.add_code_field(
            format!("{func_name}(value)"),
            format!("self {{ {} }}", wrap_tf_type(name, resource_type, tf_name)),
        );
    }
}

impl JsonnetRenderer {
    pub fn new() -> Self {
        Self {
            ..Default::default()
        }
    }
    pub fn add_line(&mut self, line: impl Into<String>) {
        self.lines.push(line.into())
    }

    pub fn render(&self) -> String {
        self.lines.join("\n")
    }
}
