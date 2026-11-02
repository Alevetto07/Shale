//! Resolve a Cura container stack to the literal strings CuraEngine expects.
//!
//! Cura's desktop app evaluates `value` formulas in Uranium and sends the
//! results over Arcus. The engine's own `-j` loader ignores `value` and reads
//! `default_value` only, so a frontend that wants real profiles has to resolve
//! them first. Shale walks inheritance, applies the variant, material, quality,
//! and optional intent, then evaluates formulas. Anything the formula subset
//! cannot evaluate falls back to `default_value` and is reported as a warning.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::catalog::{specificity, Catalog};
use crate::definition::RawSetting;
use crate::formula::{self, Expr, FormulaError, FormulaHost};
use crate::value::Value;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UserSettings {
    pub layer_height: Option<f64>,
    pub infill_sparse_density: Option<f64>,
    pub support_enable: Option<bool>,
    pub material_print_temperature: Option<f64>,
    pub material_bed_temperature: Option<f64>,
    /// Extra literal overrides (`"key" = "value"`) applied last.
    pub extra: Vec<(String, String)>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ResolveRequest {
    pub printer_id: String,
    pub variant_name: Option<String>,
    pub quality_type: Option<String>,
    pub material_id: Option<String>,
    /// Reserved for intent profiles. `None` leaves them out of the stack.
    pub intent_category: Option<String>,
    pub settings: UserSettings,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProfileSummary {
    pub printer_name: String,
    pub machine_width: f64,
    pub machine_depth: f64,
    pub machine_height: f64,
    pub machine_center_is_zero: bool,
    pub layer_height: f64,
    pub line_width: f64,
    pub infill_sparse_density: f64,
    pub infill_line_distance: f64,
    pub support_enable: bool,
    pub nozzle_temperature: f64,
    pub bed_temperature: f64,
    pub nozzle_size: f64,
    pub adhesion_type: String,
    pub speed_print: f64,
    pub material_diameter: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ResolvedSettings {
    pub printer_id: String,
    pub printer_name: String,
    pub variant_name: String,
    pub quality_type: String,
    pub material_id: String,
    pub global: std::collections::BTreeMap<String, String>,
    pub extruders: Vec<std::collections::BTreeMap<String, String>>,
    pub summary: ProfileSummary,
    pub warnings: Vec<String>,
    pub warning_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingType {
    Float,
    Int,
    Bool,
    Str,
    Enum,
    Extruder,
    Polygon,
}

impl SettingType {
    fn parse(raw: Option<&str>) -> Self {
        match raw.unwrap_or("") {
            "float" | "optional_float" => Self::Float,
            "int" | "[int]" => Self::Int,
            "bool" => Self::Bool,
            "enum" | "optional_enum" => Self::Enum,
            "extruder" | "optional_extruder" => Self::Extruder,
            "polygon" | "polygons" => Self::Polygon,
            _ => Self::Str,
        }
    }
}

#[derive(Clone, Debug)]
enum SettingExpr {
    Literal(Value),
    Formula(Expr),
}

#[derive(Clone, Debug)]
struct SettingSpec {
    ty: SettingType,
    fallback: Value,
    value: SettingExpr,
    resolve: Option<Expr>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Scope {
    Global,
    Extruder(u8),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Mode {
    Value,
    Resolve,
}

struct EvalCtx {
    global: HashMap<String, SettingSpec>,
    extruders: Vec<HashMap<String, SettingSpec>>,
    current: Scope,
    cache: HashMap<(Scope, String, Mode), Value>,
    stack: HashSet<(Scope, String, Mode)>,
    warnings: Vec<String>,
    warning_count: usize,
}

impl Catalog {
    pub fn resolve(
        &self,
        request: &ResolveRequest,
    ) -> Result<ResolvedSettings, crate::error::Error> {
        let printer = self
            .printer(request.printer_id.as_str())
            .ok_or_else(|| crate::error::Error::UnknownPrinter(request.printer_id.clone()))?;
        let printer_name = printer.name.clone();
        let preferred_variant = printer.preferred_variant_name.clone();
        let preferred_quality = printer.preferred_quality_type.clone();
        let preferred_material = printer.preferred_material.clone();
        let trains = printer.extruder_trains.clone();
        let ancestry = self.ancestry_ids(&printer.id).ok_or_else(|| {
            crate::error::Error::message(format!(
                "could not resolve inheritance for {}",
                printer.id
            ))
        })?;

        let mut global = compile_settings(&self.merged_settings(&printer.id)?)?;
        let extruder_id = trains
            .first()
            .map(|(_, id)| id.clone())
            .unwrap_or_else(|| "fdmextruder".to_string());
        let mut extruder = if self.definitions.contains_key(&extruder_id) {
            compile_settings(&self.merged_settings(&extruder_id)?)?
        } else {
            HashMap::new()
        };

        let variant_name = request
            .variant_name
            .clone()
            .or(preferred_variant)
            .unwrap_or_else(|| "0.4mm Nozzle".to_string());
        let quality_type = request
            .quality_type
            .clone()
            .or(preferred_quality)
            .unwrap_or_else(|| "normal".to_string());
        let mut warnings = Vec::new();

        apply_variants(
            self,
            &ancestry,
            &variant_name,
            &mut global,
            &mut extruder,
            &mut warnings,
        );

        let diameter = literal_number(&global, "material_diameter")
            .or_else(|| literal_number(&extruder, "material_diameter"))
            .unwrap_or(1.75);
        let material_id = select_material_id(
            self,
            request
                .material_id
                .as_deref()
                .or(preferred_material.as_deref()),
            diameter,
        );
        if let Some(id) = &material_id {
            apply_material(
                self,
                id,
                &ancestry,
                nozzle_hint(&global, &variant_name),
                &mut global,
                &mut extruder,
            );
        }

        apply_qualities(
            self,
            &ancestry,
            &quality_type,
            Some(variant_name.as_str()),
            material_id.as_deref(),
            &mut global,
            &mut extruder,
            &mut warnings,
        );
        if let Some(intent) = &request.intent_category {
            apply_intents(
                self,
                &ancestry,
                intent,
                &quality_type,
                Some(variant_name.as_str()),
                material_id.as_deref(),
                &mut global,
                &mut extruder,
                &mut warnings,
            );
        }

        apply_user(&request.settings, &mut global, &mut extruder);

        let mut ctx = EvalCtx {
            global,
            extruders: vec![extruder],
            current: Scope::Global,
            cache: HashMap::new(),
            stack: HashSet::new(),
            warnings,
            warning_count: 0,
        };
        ctx.warning_count = ctx.warnings.len();

        let mut global_values = evaluate_scope(&mut ctx, Scope::Global, Mode::Resolve);
        let extruder_values = evaluate_scope(&mut ctx, Scope::Extruder(0), Mode::Value);
        // The engine looks some extruder-only keys up on the global stack. In
        // particular a setting value of -1 means "the current extruder" and the
        // engine then reads `extruder_nr` from whichever container it is in.
        for (key, value) in &extruder_values {
            global_values
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        global_values.insert("extruder_nr".to_string(), "0".to_string());
        // Cura's frontend substitutes `{setting}` inside start and end G-code
        // before the engine sees it. The engine writes those strings through.
        let global_snapshot = global_values.clone();
        substitute_placeholders(&mut global_values, &global_snapshot);
        let mut extruder_owned = extruder_values;
        substitute_placeholders(&mut extruder_owned, &global_values);
        let extruder_values = extruder_owned;
        let summary = summarise(&printer_name, &global_values, &extruder_values);
        let warning_count = ctx.warning_count;
        let mut warnings = ctx.warnings;
        warnings.truncate(40);

        Ok(ResolvedSettings {
            printer_id: request.printer_id.clone(),
            printer_name,
            variant_name,
            quality_type,
            material_id: material_id.unwrap_or_default(),
            global: global_values,
            extruders: vec![extruder_values],
            summary,
            warnings,
            warning_count,
        })
    }
}

fn compile_settings(
    raw: &HashMap<String, RawSetting>,
) -> Result<HashMap<String, SettingSpec>, crate::error::Error> {
    let mut compiled = HashMap::with_capacity(raw.len());
    for (key, setting) in raw {
        compiled.insert(key.clone(), compile_setting(setting));
    }
    Ok(compiled)
}

fn compile_setting(raw: &RawSetting) -> SettingSpec {
    let fallback = raw
        .default_value
        .as_ref()
        .map(json_to_value)
        .unwrap_or(Value::Null);
    let value = match &raw.value {
        None => SettingExpr::Literal(fallback.clone()),
        Some(json) => match compile_json_expr(json) {
            Ok(expr) => expr,
            // Keep a call that fails at evaluation time so the fallback is used
            // and the setting shows up in the warning list.
            Err(_) => SettingExpr::Formula(Expr::Call {
                func: "unsupported_formula".into(),
                args: Vec::new(),
                keywords: Vec::new(),
            }),
        },
    };
    let resolve = match &raw.resolve {
        Some(Json::String(source)) => formula::parse(source).ok(),
        _ => None,
    };
    SettingSpec {
        ty: SettingType::parse(raw.ty.as_deref()),
        fallback,
        value,
        resolve,
    }
}

fn compile_json_expr(json: &Json) -> Result<SettingExpr, FormulaError> {
    match json {
        Json::String(source) => {
            if source.trim().is_empty() {
                Ok(SettingExpr::Literal(Value::Str(String::new())))
            } else {
                Ok(SettingExpr::Formula(formula::parse(source)?))
            }
        }
        other => Ok(SettingExpr::Literal(json_to_value(other))),
    }
}

fn json_to_value(json: &Json) -> Value {
    match json {
        Json::Null => Value::Null,
        Json::Bool(value) => Value::Bool(*value),
        Json::Number(value) => Value::Num(value.as_f64().unwrap_or(0.0)),
        Json::String(value) => Value::Str(value.clone()),
        Json::Array(items) => Value::List(items.iter().map(json_to_value).collect()),
        Json::Object(_) => Value::Null,
    }
}

fn apply_variants(
    catalog: &Catalog,
    ancestry: &[String],
    variant_name: &str,
    global: &mut HashMap<String, SettingSpec>,
    extruder: &mut HashMap<String, SettingSpec>,
    warnings: &mut Vec<String>,
) {
    let mut matches: Vec<&crate::instance::InstanceFile> = catalog
        .instances
        .iter()
        .filter(|instance| instance.type_name == "variant" && instance.name == variant_name)
        .filter(|instance| ancestry.iter().any(|id| id == &instance.definition))
        .collect();
    matches.sort_by_key(|instance| specificity(&instance.definition, ancestry));
    for instance in matches {
        apply_instance_values(&instance.values, global, warnings);
        apply_instance_values(&instance.values, extruder, warnings);
    }
}

fn apply_qualities(
    catalog: &Catalog,
    ancestry: &[String],
    quality_type: &str,
    variant: Option<&str>,
    material: Option<&str>,
    global: &mut HashMap<String, SettingSpec>,
    extruder: &mut HashMap<String, SettingSpec>,
    warnings: &mut Vec<String>,
) {
    let mut matches: Vec<&crate::instance::InstanceFile> = catalog
        .instances
        .iter()
        .filter(|instance| instance.type_name == "quality")
        .filter(|instance| instance.quality_type.as_deref() == Some(quality_type))
        .filter(|instance| {
            ancestry.iter().any(|id| id == &instance.definition)
                || is_known_extruder(catalog, &instance.definition, ancestry)
        })
        .filter(|instance| variant_ok(instance.variant.as_deref(), variant))
        .filter(|instance| material_ok(instance.material.as_deref(), material))
        .collect();
    matches.sort_by_key(|instance| (specificity(&instance.definition, ancestry), instance.weight));
    for instance in matches {
        let extruder_profile = catalog
            .definitions
            .get(&instance.definition)
            .map(|definition| definition.is_extruder)
            .unwrap_or(false);
        if instance.global_quality || !extruder_profile {
            apply_instance_values(&instance.values, global, warnings);
        }
        if extruder_profile || !instance.global_quality {
            apply_instance_values(&instance.values, extruder, warnings);
        }
    }
}

fn apply_intents(
    catalog: &Catalog,
    ancestry: &[String],
    intent: &str,
    quality_type: &str,
    variant: Option<&str>,
    material: Option<&str>,
    global: &mut HashMap<String, SettingSpec>,
    extruder: &mut HashMap<String, SettingSpec>,
    warnings: &mut Vec<String>,
) {
    let mut matches: Vec<&crate::instance::InstanceFile> = catalog
        .instances
        .iter()
        .filter(|instance| instance.type_name == "intent")
        .filter(|instance| instance.intent_category.as_deref() == Some(intent))
        .filter(|instance| {
            instance.quality_type.as_deref() == Some(quality_type)
                || instance.quality_type.is_none()
        })
        .filter(|instance| {
            ancestry.iter().any(|id| id == &instance.definition)
                || is_known_extruder(catalog, &instance.definition, ancestry)
        })
        .filter(|instance| variant_ok(instance.variant.as_deref(), variant))
        .filter(|instance| material_ok(instance.material.as_deref(), material))
        .collect();
    matches.sort_by_key(|instance| specificity(&instance.definition, ancestry));
    for instance in matches {
        apply_instance_values(&instance.values, global, warnings);
        apply_instance_values(&instance.values, extruder, warnings);
    }
}

fn is_known_extruder(catalog: &Catalog, definition_id: &str, ancestry: &[String]) -> bool {
    catalog
        .definitions
        .get(definition_id)
        .map(|definition| definition.is_extruder)
        .unwrap_or(false)
        && catalog
            .definitions
            .get(definition_id)
            .is_some_and(|definition| {
                definition.inherits.as_deref().is_some_and(|parent| {
                    ancestry.iter().any(|id| id == parent) || parent == "fdmextruder"
                }) || ancestry.iter().any(|id| {
                    catalog.definitions.get(id).is_some_and(|machine| {
                        machine
                            .extruder_trains
                            .iter()
                            .any(|(_, train)| train == definition_id)
                    })
                })
            })
}

fn variant_ok(profile_variant: Option<&str>, selected: Option<&str>) -> bool {
    match (profile_variant, selected) {
        (None, _) => true,
        (Some(profile), Some(selected)) => profile == selected,
        (Some(_), None) => false,
    }
}

fn material_ok(profile_material: Option<&str>, selected: Option<&str>) -> bool {
    match (profile_material, selected) {
        (None, _) => true,
        (Some(profile), Some(selected)) => {
            profile == selected
                || profile.strip_suffix("_175") == Some(selected)
                || profile.strip_suffix("_285") == Some(selected)
                || selected.strip_suffix("_175") == Some(profile)
                || selected.strip_suffix("_285") == Some(profile)
        }
        (Some(_), None) => false,
    }
}

fn apply_instance_values(
    values: &HashMap<String, String>,
    specs: &mut HashMap<String, SettingSpec>,
    warnings: &mut Vec<String>,
) {
    for (key, raw) in values {
        match compile_cfg_value(raw) {
            Ok(expr) => {
                let spec = specs.entry(key.clone()).or_insert_with(|| SettingSpec {
                    ty: SettingType::Float,
                    fallback: Value::Null,
                    value: SettingExpr::Literal(Value::Null),
                    resolve: None,
                });
                if matches!(spec.fallback, Value::Null) {
                    if let SettingExpr::Literal(value) = &expr {
                        spec.fallback = value.clone();
                    }
                }
                spec.value = expr;
            }
            Err(err) => {
                if warnings.len() < 40 {
                    warnings.push(format!("{key}: {err}"));
                }
            }
        }
    }
}

fn compile_cfg_value(raw: &str) -> Result<SettingExpr, FormulaError> {
    let trimmed = raw.trim();
    if let Some(formula) = trimmed.strip_prefix('=') {
        let formula = formula.trim();
        if formula.is_empty() {
            return Ok(SettingExpr::Literal(Value::Str(String::new())));
        }
        return Ok(SettingExpr::Formula(formula::parse(formula)?));
    }
    Ok(SettingExpr::Literal(parse_literal(trimmed)))
}

fn parse_literal(text: &str) -> Value {
    if text == "True" || text == "true" {
        return Value::Bool(true);
    }
    if text == "False" || text == "false" {
        return Value::Bool(false);
    }
    if text == "None" || text.is_empty() {
        return Value::Null;
    }
    if let Ok(number) = text.parse::<f64>() {
        if text
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_digit() || ch == '-' || ch == '+' || ch == '.')
        {
            return Value::Num(number);
        }
    }
    Value::Str(text.to_string())
}

fn select_material_id(catalog: &Catalog, wanted: Option<&str>, diameter: f64) -> Option<String> {
    let wanted = wanted?;
    let mut best: Option<(&crate::material::Material, f64)> = None;
    for material in &catalog.materials {
        if !material_ok(Some(&material.id), Some(wanted)) && material.id != wanted {
            continue;
        }
        let distance = (material.diameter - diameter).abs();
        if best
            .as_ref()
            .map(|(_, existing)| distance < *existing)
            .unwrap_or(true)
        {
            best = Some((material, distance));
        }
    }
    best.map(|(material, _)| material.id.clone())
}

fn apply_material(
    catalog: &Catalog,
    material_id: &str,
    ancestry: &[String],
    nozzle: Option<f64>,
    global: &mut HashMap<String, SettingSpec>,
    extruder: &mut HashMap<String, SettingSpec>,
) {
    let Some(material) = catalog
        .materials
        .iter()
        .find(|material| material.id == material_id)
    else {
        return;
    };
    let mut values = material.settings.clone();
    for machine in &material.machines {
        let matches = machine
            .products
            .iter()
            .any(|product| ancestry.iter().any(|id| id == product));
        if !matches {
            continue;
        }
        values.extend(machine.settings.clone());
        if let Some(nozzle) = nozzle {
            let needle = format_nozzle(nozzle);
            if let Some(hotend) = machine
                .hotends
                .iter()
                .find(|hotend| hotend.id.contains(&needle))
            {
                values.extend(hotend.settings.clone());
            }
        }
    }
    values.insert("material_diameter".to_string(), material.diameter);
    for (key, value) in values {
        set_literal(global, &key, Value::Num(value));
        set_literal(extruder, &key, Value::Num(value));
    }
}

fn apply_user(
    settings: &UserSettings,
    global: &mut HashMap<String, SettingSpec>,
    extruder: &mut HashMap<String, SettingSpec>,
) {
    if let Some(layer) = settings.layer_height {
        force_literal(global, "layer_height", Value::Num(layer));
        force_literal(global, "layer_height_0", Value::Num(layer));
        force_literal(extruder, "layer_height", Value::Num(layer));
        force_literal(extruder, "layer_height_0", Value::Num(layer));
    }
    if let Some(infill) = settings.infill_sparse_density {
        force_literal(global, "infill_sparse_density", Value::Num(infill));
        force_literal(extruder, "infill_sparse_density", Value::Num(infill));
    }
    if let Some(enabled) = settings.support_enable {
        force_literal(global, "support_enable", Value::Bool(enabled));
        force_literal(extruder, "support_enable", Value::Bool(enabled));
    }
    if let Some(temperature) = settings.material_print_temperature {
        for key in [
            "material_print_temperature",
            "material_print_temperature_layer_0",
            "default_material_print_temperature",
        ] {
            force_literal(global, key, Value::Num(temperature));
            force_literal(extruder, key, Value::Num(temperature));
        }
    }
    if let Some(temperature) = settings.material_bed_temperature {
        for key in [
            "material_bed_temperature",
            "material_bed_temperature_layer_0",
            "default_material_bed_temperature",
        ] {
            force_literal(global, key, Value::Num(temperature));
            force_literal(extruder, key, Value::Num(temperature));
        }
    }
    for (key, value) in &settings.extra {
        let literal = parse_literal(value);
        force_literal(global, key, literal.clone());
        force_literal(extruder, key, literal);
    }
}

fn set_literal(specs: &mut HashMap<String, SettingSpec>, key: &str, value: Value) {
    let spec = specs.entry(key.to_string()).or_insert_with(|| SettingSpec {
        ty: SettingType::Float,
        fallback: value.clone(),
        value: SettingExpr::Literal(value.clone()),
        resolve: None,
    });
    spec.value = SettingExpr::Literal(value);
}

fn force_literal(specs: &mut HashMap<String, SettingSpec>, key: &str, value: Value) {
    let spec = specs.entry(key.to_string()).or_insert_with(|| SettingSpec {
        ty: inferred_type(&value),
        fallback: value.clone(),
        value: SettingExpr::Literal(value.clone()),
        resolve: None,
    });
    spec.value = SettingExpr::Literal(value.clone());
    spec.resolve = None;
    if matches!(spec.fallback, Value::Null) {
        spec.fallback = value;
    }
}

fn inferred_type(value: &Value) -> SettingType {
    match value {
        Value::Bool(_) => SettingType::Bool,
        Value::Num(_) => SettingType::Float,
        _ => SettingType::Str,
    }
}

fn literal_number(specs: &HashMap<String, SettingSpec>, key: &str) -> Option<f64> {
    match specs.get(key).map(|spec| &spec.value) {
        Some(SettingExpr::Literal(Value::Num(value))) => Some(*value),
        _ => None,
    }
}

fn nozzle_hint(specs: &HashMap<String, SettingSpec>, variant_name: &str) -> Option<f64> {
    literal_number(specs, "machine_nozzle_size").or_else(|| {
        let digits: String = variant_name
            .chars()
            .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
            .collect();
        // "0.4mm Nozzle" starts with the number. "Nozzle 0.4" does not.
        digits.parse().ok().or_else(|| {
            variant_name
                .split(|ch: char| !ch.is_ascii_digit() && ch != '.')
                .find_map(|part| part.parse::<f64>().ok())
        })
    })
}

fn format_nozzle(size: f64) -> String {
    let text = format!("{size:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn evaluate_scope(
    ctx: &mut EvalCtx,
    scope: Scope,
    mode: Mode,
) -> std::collections::BTreeMap<String, String> {
    let keys: Vec<String> = match scope {
        Scope::Global => ctx.global.keys().cloned().collect(),
        Scope::Extruder(index) => ctx
            .extruders
            .get(index as usize)
            .map(|specs| specs.keys().cloned().collect())
            .unwrap_or_default(),
    };
    let mut out = std::collections::BTreeMap::new();
    for key in keys {
        match ctx.eval_setting(scope, &key, mode) {
            Ok(value) => {
                let ty = ctx
                    .spec(scope, &key)
                    .map(|spec| spec.ty)
                    .unwrap_or(SettingType::Str);
                out.insert(key, serialise(ty, &value));
            }
            Err(err) => {
                ctx.warn(format!("{key}: {err}"));
                let fallback = ctx.fallback(scope, &key);
                let ty = ctx
                    .spec(scope, &key)
                    .map(|spec| spec.ty)
                    .unwrap_or(SettingType::Str);
                out.insert(key, serialise(ty, &fallback));
            }
        }
    }
    out
}

impl EvalCtx {
    fn eval_setting(
        &mut self,
        scope: Scope,
        key: &str,
        requested: Mode,
    ) -> Result<Value, FormulaError> {
        let Some((home, mode)) = self.locate(scope, key, requested) else {
            return Err(FormulaError::new(format!("unknown setting {key}")));
        };
        let cache_key = (home, key.to_string(), mode);
        if let Some(value) = self.cache.get(&cache_key) {
            return Ok(value.clone());
        }
        if !self.stack.insert(cache_key.clone()) {
            let fallback = self.fallback(home, key);
            self.warn(format!("cycle while resolving {key}"));
            self.cache.insert(cache_key, fallback.clone());
            return Ok(fallback);
        }
        let expr = self.expression(home, key, mode);
        let previous = self.current;
        self.current = home;
        let result = match &expr {
            SettingExpr::Literal(value) => Ok(value.clone()),
            SettingExpr::Formula(formula) => formula::eval(formula, self),
        };
        self.current = previous;
        self.stack.remove(&cache_key);
        let value = match result {
            Ok(value) => value,
            Err(err) => {
                self.warn(format!("{key}: {err}"));
                self.fallback(home, key)
            }
        };
        self.cache.insert(cache_key, value.clone());
        Ok(value)
    }

    fn locate(&self, scope: Scope, key: &str, requested: Mode) -> Option<(Scope, Mode)> {
        let home = self.find_home(scope, key)?;
        let spec = self.spec(home, key)?;
        let mode = if requested == Mode::Resolve && spec.resolve.is_some() {
            Mode::Resolve
        } else {
            Mode::Value
        };
        Some((home, mode))
    }

    fn find_home(&self, scope: Scope, key: &str) -> Option<Scope> {
        match scope {
            Scope::Extruder(index) => {
                if self
                    .extruders
                    .get(index as usize)
                    .is_some_and(|specs| specs.contains_key(key))
                {
                    Some(Scope::Extruder(index))
                } else if self.global.contains_key(key) {
                    Some(Scope::Global)
                } else {
                    None
                }
            }
            Scope::Global => {
                if self.global.contains_key(key) {
                    Some(Scope::Global)
                } else if self
                    .extruders
                    .first()
                    .is_some_and(|specs| specs.contains_key(key))
                {
                    Some(Scope::Extruder(0))
                } else {
                    None
                }
            }
        }
    }

    fn spec(&self, scope: Scope, key: &str) -> Option<&SettingSpec> {
        match scope {
            Scope::Global => self.global.get(key),
            Scope::Extruder(index) => self.extruders.get(index as usize)?.get(key),
        }
    }

    fn expression(&self, scope: Scope, key: &str, mode: Mode) -> SettingExpr {
        let Some(spec) = self.spec(scope, key) else {
            return SettingExpr::Literal(Value::Null);
        };
        if mode == Mode::Resolve {
            if let Some(formula) = &spec.resolve {
                return SettingExpr::Formula(formula.clone());
            }
        }
        spec.value.clone()
    }

    fn fallback(&self, scope: Scope, key: &str) -> Value {
        let Some(spec) = self.spec(scope, key) else {
            return Value::Num(0.0);
        };
        if !matches!(spec.fallback, Value::Null) {
            return spec.fallback.clone();
        }
        match spec.ty {
            SettingType::Bool => Value::Bool(false),
            SettingType::Str | SettingType::Enum => Value::Str(String::new()),
            _ => Value::Num(0.0),
        }
    }

    fn warn(&mut self, message: String) {
        self.warning_count += 1;
        if self.warnings.len() < 50 {
            self.warnings.push(message);
        }
    }
}

impl FormulaHost for EvalCtx {
    fn setting(&mut self, name: &str) -> Result<Value, FormulaError> {
        let scope = self.current;
        self.eval_setting(scope, name, Mode::Value)
    }

    fn extruder_value(&mut self, index: i32, key: &str) -> Result<Value, FormulaError> {
        let index = if index < 0 { 0 } else { index as u8 };
        let scope = if (index as usize) < self.extruders.len() {
            Scope::Extruder(index)
        } else {
            Scope::Extruder(0)
        };
        self.eval_setting(scope, key, Mode::Value)
    }

    fn extruder_values(&mut self, key: &str) -> Result<Value, FormulaError> {
        let count = self.extruders.len().max(1);
        let mut items = Vec::with_capacity(count);
        for index in 0..count {
            items.push(self.eval_setting(Scope::Extruder(index as u8), key, Mode::Value)?);
        }
        Ok(Value::List(items))
    }

    fn resolve_or_value(&mut self, key: &str) -> Result<Value, FormulaError> {
        let scope = self.current;
        self.eval_setting(scope, key, Mode::Resolve)
    }

    fn any_extruder_with_material(&mut self, key: &str) -> Result<Value, FormulaError> {
        let count = self.extruders.len();
        for index in 0..count {
            if let Ok(value) = self.eval_setting(Scope::Extruder(index as u8), key, Mode::Value) {
                if is_affirmative(&value) {
                    return Ok(Value::Num(index as f64));
                }
            }
        }
        Ok(Value::Num(-1.0))
    }

    fn default_extruder_position(&mut self) -> Result<Value, FormulaError> {
        Ok(Value::Num(0.0))
    }
}

fn is_affirmative(value: &Value) -> bool {
    match value {
        Value::Bool(value) => *value,
        Value::Num(value) => *value != 0.0,
        Value::Str(value) => matches!(
            value.to_ascii_lowercase().as_str(),
            "true" | "yes" | "on" | "1"
        ),
        _ => false,
    }
}

fn serialise(ty: SettingType, value: &Value) -> String {
    match ty {
        SettingType::Bool => {
            if is_affirmative(value) {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        SettingType::Int | SettingType::Extruder => match value {
            Value::Num(number) => format!("{}", number.trunc() as i64),
            Value::Bool(true) => "1".to_string(),
            Value::Bool(false) => "0".to_string(),
            Value::Str(text) => text.clone(),
            _ => "0".to_string(),
        },
        SettingType::Polygon => format_value(value),
        SettingType::Float => match value.as_number() {
            Some(number) => format_float(number),
            None => format_value(value),
        },
        SettingType::Str | SettingType::Enum => match value {
            Value::Str(text) => text.clone(),
            Value::Bool(value) => {
                if *value {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            Value::Num(number) => format_float(*number),
            other => format_value(other),
        },
    }
}

fn format_value(value: &Value) -> String {
    match value {
        Value::List(items) => {
            let parts: Vec<String> = items.iter().map(format_value).collect();
            format!("[{}]", parts.join(","))
        }
        Value::Num(number) => format_float(*number),
        Value::Bool(value) => {
            if *value {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        Value::Str(text) => text.clone(),
        Value::Null => String::new(),
    }
}

fn format_float(number: f64) -> String {
    if !number.is_finite() {
        return if number.is_sign_positive() {
            "inf".to_string()
        } else {
            "-inf".to_string()
        };
    }
    let text = format!("{number:.6}");
    if !text.contains('.') {
        return text;
    }
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn substitute_placeholders(
    values: &mut std::collections::BTreeMap<String, String>,
    fallback: &std::collections::BTreeMap<String, String>,
) {
    for _ in 0..6 {
        let snapshot = values.clone();
        let mut changed = false;
        for value in values.values_mut() {
            if !value.contains('{') {
                continue;
            }
            let next = replace_placeholders(value, &snapshot, fallback);
            if next != *value {
                *value = next;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

fn replace_placeholders(
    input: &str,
    primary: &std::collections::BTreeMap<String, String>,
    fallback: &std::collections::BTreeMap<String, String>,
) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('{') {
        output.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(end) = after.find('}') {
            let key = &after[..end];
            if is_setting_key(key) {
                if let Some(replacement) = primary.get(key).or_else(|| fallback.get(key)) {
                    if !replacement.contains('{') {
                        output.push_str(replacement);
                        rest = &after[end + 1..];
                        continue;
                    }
                }
            }
        }
        output.push('{');
        rest = &rest[start + 1..];
    }
    output.push_str(rest);
    output
}

fn is_setting_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn summarise(
    printer_name: &str,
    global: &std::collections::BTreeMap<String, String>,
    extruder: &std::collections::BTreeMap<String, String>,
) -> ProfileSummary {
    let pick = |key: &str| -> String {
        extruder
            .get(key)
            .or_else(|| global.get(key))
            .cloned()
            .unwrap_or_default()
    };
    let num = |key: &str, default: f64| -> f64 { pick(key).parse().unwrap_or(default) };
    let flag = |key: &str| -> bool {
        matches!(
            pick(key).to_ascii_lowercase().as_str(),
            "true" | "yes" | "on" | "1"
        )
    };
    ProfileSummary {
        printer_name: printer_name.to_string(),
        machine_width: num("machine_width", 220.0),
        machine_depth: num("machine_depth", 220.0),
        machine_height: num("machine_height", 250.0),
        machine_center_is_zero: flag("machine_center_is_zero"),
        layer_height: num("layer_height", 0.2),
        line_width: num("line_width", num("machine_nozzle_size", 0.4)),
        infill_sparse_density: num("infill_sparse_density", 20.0),
        infill_line_distance: num("infill_line_distance", 2.0),
        support_enable: flag("support_enable"),
        nozzle_temperature: num("material_print_temperature", 200.0),
        bed_temperature: num("material_bed_temperature", 60.0),
        nozzle_size: num("machine_nozzle_size", 0.4),
        adhesion_type: {
            let value = pick("adhesion_type");
            if value.is_empty() {
                "skirt".to_string()
            } else {
                value
            }
        },
        speed_print: num("speed_print", 50.0),
        material_diameter: num("material_diameter", 1.75),
    }
}
