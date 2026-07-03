use clap::Parser;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use std::{
    collections::HashMap,
    fs::{self, File, create_dir_all},
    io::Write,
    path::Path,
    process::Command,
};
use terraform_wrapper::{Terraform, TerraformCommand, prelude::ProvidersCommand};
use which::which;

use anyhow::Result;
use convert_case::{Case, Casing};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ProviderSchemas {
    pub format_version: String,
    pub provider_schemas: HashMap<String, ProviderSchema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProviderSchema {
    pub provider: Option<Schema>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub resource_schemas: HashMap<String, Schema>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub data_source_schemas: HashMap<String, Schema>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub ephemeral_resource_schemas: HashMap<String, Schema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Schema {
    pub version: u64,
    pub block: Block,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Block {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub attributes: HashMap<String, Attribute>,
    pub block_types: Option<HashMap<String, BlockType>>,
    pub description: Option<String>,
    pub description_kind: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Attribute {
    #[serde(rename = "type")]
    pub attr_type: Option<AttributeType>,
    pub description: Option<String>,
    pub description_kind: Option<String>,
    pub required: Option<bool>,
    pub optional: Option<bool>,
    pub computed: Option<bool>,
    pub sensitive: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AttributeType {
    Primitive(String),               // "string", "number", "bool"
    Complex(Vec<serde_json::Value>), // ["list", "string"], ["map", "number"], etc.
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlockType {
    pub nesting_mode: Option<String>, // "single", "list", "set", "map"
    pub block: Option<Block>,
    pub min_items: Option<u64>,
    pub max_items: Option<u64>,
}

#[derive(Debug, Default)]
pub struct JsonnetStructure {
    pub providers: HashMap<String, JsonnetComponents>,
}

#[derive(Debug, Default)]
pub struct JsonnetComponents {
    // Name: Content
    pub data: HashMap<String, String>,
    pub resource: HashMap<String, String>,
}

fn wrap_tf_type(name: &str, resource_type: &str, tf_name: &str) -> String {
    wrap_tf_type_named(name, resource_type, tf_name, "value")
}

fn wrap_tf_type_named(
    name: &str,
    resource_type: &str,
    tf_resource_name: &str,
    val_var_name: &str,
) -> String {
    format!(
        r#"{resource_type}+: {{
            {tf_resource_name}+: {{ [terraformName]+: {{ '{name}': {val_var_name} }} }},
        }},"#
    )
}

impl Block {
    fn to_jsonnet(&self, name: &str, resource_type: &str) -> String {
        let mut lines = vec![];
        let mut args = vec!["terraformName".to_string()];
        let required: Vec<String> = self
            .attributes
            .iter()
            .filter_map(|(name, attr)| attr.required.and(Some(name.to_string())))
            .collect();
        args.extend(required.clone());
        lines.push("{".to_string());
        lines.push(format!("new({}):: {{", args.join(", ")));
        lines.push("_type:: 'tf',".to_string());
        lines.push(format!("{resource_type}+: {{"));
        lines.push(format!("{name}+: {{ [terraformName]+: {{"));
        for arg in required {
            lines.push(format!("'{arg}': {arg},"));
        }
        lines.push("}}},".to_string());
        for (arg_name, attr) in &self.attributes {
            lines.push(attr.to_jsonnet(arg_name, resource_type, name));
        }
        lines.push("},".to_string());
        lines.push("}".to_string());
        lines.join("\n")
    }
}

impl Attribute {
    fn to_jsonnet(&self, name: &str, resource_type: &str, tf_name: &str) -> String {
        let mut lines = vec![];
        let func_name = format!("with_{name}").to_case(Case::Camel);
        if !self.description.clone().unwrap_or_default().is_empty() {
            lines.push(self.to_doc(&func_name));
        }
        lines.push(format!("{func_name}(value):: self {{"));
        lines.push(wrap_tf_type(name, resource_type, tf_name));
        lines.push("},".to_string());
        lines.join("\n")
    }

    fn to_doc(&self, name: &str) -> String {
        format!(
            "'#{name}': {{ 'function': {{ help: |||\n {} \n||| }} }},",
            self.description.clone().unwrap_or_default()
        )
    }
}

fn write_jsonnet(dir: impl AsRef<Path>, name: &str, value: &str) {
    let filename = dir.as_ref().join(format!("{name}.libsonnet"));
    let p = Path::new(&filename);
    create_dir_all(p.parent().unwrap()).unwrap();
    let mut file = File::create(p).unwrap();
    file.write_all(value.as_bytes()).unwrap();
    Command::new("jsonnetfmt")
        .arg("-i")
        .arg(&filename)
        .output()
        .unwrap_or_else(|_| {
            panic!(
                "Running fmt on file {}",
                filename.to_str().unwrap_or_default()
            )
        });
}

fn write_import_file(dir: impl AsRef<Path>) -> Result<()> {
    let paths = fs::read_dir(dir.as_ref())?;
    let mut imports: HashMap<String, String> = HashMap::new();
    for path in paths {
        let path = path?;
        if path.path().is_dir() {
            let sub_module_file = path.path().join("modules.libsonnet");
            let dirname = path.file_name().to_str().unwrap().to_string();
            if sub_module_file.exists() {
                imports.insert(format!("{dirname}/modules.libsonnet"), dirname);
            }
        } else {
            let filename = path.file_name().to_str().unwrap().to_string();
            if filename != "modules.libsonnet" {
                let name = filename.rsplit_once(".").unwrap().0.to_case(Case::Camel);
                imports.insert(filename, name);
            }
        }
    }
    let mut lines = vec![];
    lines.push("{".to_string());
    for (filename, name) in imports {
        lines.push(format!("  {}:: (import '{}'),", name, filename,));
    }

    lines.push("}".to_string());
    write_jsonnet(dir, "modules", &lines.join("\n"));
    Ok(())
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Name of the person to greet
    #[arg(short, long, default_value = "out")]
    out_dir: String,

    #[arg(short, long, default_value = "tofu")]
    binary: String,

    #[arg(short, long)]
    input_schema: Option<String>,

    #[arg(short, long, default_value = ".")]
    tf_dir: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let schemas: ProviderSchemas = if let Some(input_schema) = args.input_schema {
        let json = fs::read_to_string(input_schema)?;
        serde_json::from_str(&json)?
    } else {
        let tf_builder = Terraform::builder()
            .binary(which(args.binary)?)
            .working_dir(args.tf_dir)
            .build()?;
        let res = ProvidersCommand::schema()
            .arg("-json")
            .execute(&tf_builder)
            .await?;
        serde_json::from_str(&res.to_string())?
    };

    let out_dir = Path::new(&args.out_dir);
    schemas
        .provider_schemas
        .par_iter()
        .for_each(|(provider_name, schema)| {
            let provider_name = provider_name.rsplit_once("/").unwrap().1;
            let provider_dir = out_dir.join(provider_name);
            schema
                .data_source_schemas
                .iter()
                .for_each(|(name, schema)| {
                    let dirname = provider_dir.join("data");
                    write_jsonnet(&dirname, name, &schema.block.to_jsonnet(name, "data"));
                    write_import_file(dirname).unwrap();
                });
            schema.resource_schemas.iter().for_each(|(name, schema)| {
                let dirname = provider_dir.join("resource");
                write_jsonnet(&dirname, name, &schema.block.to_jsonnet(name, "resource"));
                write_import_file(dirname).unwrap();
            });
            write_import_file(provider_dir).unwrap();
        });
    write_import_file(out_dir).unwrap();

    Ok(())
}
