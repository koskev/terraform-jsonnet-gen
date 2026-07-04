use clap::Parser;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use std::{
    collections::BTreeMap,
    fs::{self, File, create_dir_all},
    io::Write,
    path::Path,
};
use terraform_wrapper::{Terraform, TerraformCommand, prelude::ProvidersCommand};
use thiserror::Error;
use which::which;

use anyhow::{Result, anyhow};
use convert_case::{Case, Casing};
use serde::{Deserialize, Serialize};

use crate::jsonnet::JsonnetRenderer;

mod jsonnet;

#[derive(Error, Debug)]
pub enum PathError {
    #[error("No parent")]
    InvalidParent,
    #[error("Invalid name to convert to string")]
    StringConversion,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProviderSchemas {
    pub format_version: String,
    pub provider_schemas: BTreeMap<String, ProviderSchema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProviderSchema {
    pub provider: Option<Schema>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resource_schemas: BTreeMap<String, Schema>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub data_source_schemas: BTreeMap<String, Schema>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ephemeral_resource_schemas: BTreeMap<String, Schema>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Schema {
    pub version: u64,
    pub block: Block,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Block {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, Attribute>,
    pub block_types: Option<BTreeMap<String, BlockType>>,
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
    pub providers: BTreeMap<String, JsonnetComponents>,
}

#[derive(Debug, Default)]
pub struct JsonnetComponents {
    // Name: Content
    pub data: BTreeMap<String, String>,
    pub resource: BTreeMap<String, String>,
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
        let mut renderer = JsonnetRenderer::new();
        let mut args = vec!["terraformName".to_string()];
        let required: Vec<String> = self
            .attributes
            .iter()
            .filter_map(|(name, attr)| attr.required.and(Some(name.to_string())))
            .collect();
        args.extend(required.clone());
        renderer.add_line("{");
        if let Some(description) = &self.description {
            renderer.add_doc_string("new", description);
        }
        renderer.add_line("local outerSelf = self,");
        renderer.add_line(format!(
            "new({}):: self.functions(terraformName) {{",
            args.join(", ")
        ));
        renderer.add_line("ref():: outerSelf.ref(terraformName),");
        renderer.add_line("_type:: 'tf',");
        renderer.add_line(format!("{resource_type}+: {{"));
        renderer.add_line(format!("{name}+: {{ [terraformName]+: {{"));
        for arg in required {
            renderer.add_line(format!("'{arg}': {arg},"));
        }
        renderer.add_line("},");
        renderer.add_line("}}},");
        {
            renderer.add_line("functions(terraformName):: {");
            // https://developer.hashicorp.com/terraform/language/meta-arguments
            let meta_arguments = vec![
                "for_each",
                "depends_on",
                "count",
                "lifecycle",
                "provider",
                "providers",
            ];
            for meta_arg in meta_arguments {
                renderer.add_with_function(meta_arg, resource_type, name, None);
            }

            self.attributes
                .iter()
                .filter(|(_, attr)| attr.is_argument())
                .for_each(|(arg_name, attr)| {
                    renderer.add_with_function(
                        arg_name,
                        resource_type,
                        name,
                        attr.description.as_deref(),
                    );
                });
            renderer.add_line("},");
        }
        {
            renderer.add_line("ref(terraformName):: {");
            let prefix = match resource_type {
                "data" => "data.",
                _ => "",
            };

            renderer.add_line("local refSelf = self,");
            renderer.add_line(format!(
                "plain(suffix=''):: '${{ {}{}.%s%s }}' % [terraformName, suffix],",
                prefix, name
            ));
            {
                // TODO: Remove duplicate documentation and reference it instead
                renderer.add_line("fields:: {");
                self.attributes.iter().for_each(|(arg_name, attr)| {
                    if let Some(help) = &attr.description {
                        renderer.add_doc_string(arg_name, help);
                    }
                    renderer.add_line(format!(
                        "'{arg_name}'(suffix=''):: refSelf.plain('.{arg_name}%s' % suffix),"
                    ));
                });
                renderer.add_line("},");
            }

            renderer.add_line("},");
        }

        renderer.add_line("}");
        renderer.render()
    }
}

impl Attribute {
    fn is_argument(&self) -> bool {
        self.optional.unwrap_or(false) || self.required.unwrap_or(false)
    }
}

fn format_jsonnet(data: &str) -> Result<String> {
    #[cfg(feature = "integrate_jsonnetfmt")]
    {
        use grustonnet_config::FormatOptions;
        use jsonnet_bridge::{
            evaluate_error::EvaluateError,
            go::{ASTBridge, ASTBridgeImpl},
        };
        let res = ASTBridgeImpl::format_snippet(
            "".to_string(),
            data.to_string(),
            FormatOptions::default().into(),
        );
        let formatted = if !res.error_data.is_empty() {
            //value.to_string()
            return Err(EvaluateError::from(res.error_data).into());
        } else {
            String::from_utf8(res.ast_data)?
        };
        Ok(formatted)
    }
    #[cfg(not(feature = "integrate_jsonnetfmt"))]
    {
        use std::process::{Command, Stdio};

        let mut fmt_process = Command::new("jsonnetfmt")
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;

        let mut stdin = fmt_process
            .stdin
            .take()
            .ok_or(anyhow!("Unable to pipe to stdin!"))?;

        stdin.write_all(data.as_bytes())?;
        drop(stdin);
        let output = fmt_process.wait_with_output()?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

fn write_jsonnet(dir: impl AsRef<Path>, name: &str, value: &str) -> Result<()> {
    let filename = dir.as_ref().join(format!("{name}.libsonnet"));
    let formatted = format_jsonnet(value).unwrap_or(value.to_string());

    let p = Path::new(&filename);
    create_dir_all(p.parent().ok_or(PathError::InvalidParent)?)?;
    let mut file = File::create(p)?;
    file.write_all(formatted.as_bytes())?;
    Ok(())
}

fn write_import_file(dir: impl AsRef<Path>) -> Result<()> {
    let paths = fs::read_dir(dir.as_ref())?;
    let mut imports: BTreeMap<String, String> = BTreeMap::new();
    for path in paths {
        let path = path?;
        if path.path().is_dir() {
            let sub_module_file = path.path().join("modules.libsonnet");
            let dirname = path
                .file_name()
                .to_str()
                .ok_or(PathError::StringConversion)?
                .to_string();
            if sub_module_file.exists() {
                imports.insert(format!("{dirname}/modules.libsonnet"), dirname);
            }
        } else {
            let filename = path
                .file_name()
                .to_str()
                .ok_or(PathError::StringConversion)?
                .to_string();
            if filename != "modules.libsonnet" {
                let name = filename
                    .rsplit_once(".")
                    .ok_or(anyhow!("Unable to split {filename} at ."))?
                    .0
                    .to_case(Case::Camel);
                imports.insert(filename, name);
            }
        }
    }
    let mut renderer = JsonnetRenderer::new();
    renderer.add_line("{");
    for (filename, name) in imports {
        renderer.add_line(format!("  {}:: (import '{}'),", name, filename,));
    }

    renderer.add_line("}");
    write_jsonnet(dir, "modules", &renderer.render())
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
        .try_for_each(|(provider_name, schema)| {
            let provider_name = provider_name
                .rsplit_once("/")
                .ok_or(anyhow!("Unable to split provider name {}", provider_name))?
                .1;
            let provider_dir = out_dir.join(provider_name);
            generate_schemas(&schema.data_source_schemas, &provider_dir, "data")?;
            generate_schemas(&schema.resource_schemas, &provider_dir, "resource")?;
            generate_schemas(
                &schema.ephemeral_resource_schemas,
                &provider_dir,
                "ephemeral",
            )?;
            write_import_file(provider_dir)?;
            Ok::<(), anyhow::Error>(())
        })?;
    write_import_file(out_dir)?;

    Ok(())
}

fn generate_schemas(
    schemas: &BTreeMap<String, Schema>,
    provider_dir: impl AsRef<Path>,
    resource_type: &str,
) -> Result<()> {
    let resource_dirname = provider_dir.as_ref().join(resource_type);
    for (name, schema) in schemas {
        write_jsonnet(
            &resource_dirname,
            name,
            &schema.block.to_jsonnet(name, resource_type),
        )?;
    }
    let _ = write_import_file(&resource_dirname);
    Ok(())
}
