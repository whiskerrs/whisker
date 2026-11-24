//! MapLibre style expressions, legacy functions, and legacy filters.

#[cfg(test)]
use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value as Json;

use crate::style::color::{self, Rgba};
use crate::tiles::mvt::GeometryKind;
use crate::value::Value;

/// What an expression reads from the feature it is evaluated for.
pub(crate) trait FeatureView {
    fn property(&self, key: &str) -> Option<Value>;
    fn kind(&self) -> GeometryKind;
    fn id(&self) -> Option<Value>;
    fn state(&self, _key: &str) -> Option<Value> {
        None
    }
    fn properties(&self) -> Value {
        Value::Null
    }
}

pub(crate) struct Context<'a> {
    pub zoom: f64,
    pub feature: Option<&'a dyn FeatureView>,
    /// For `heatmap-color`, `line-gradient`, and terrain colors.
    pub input: Option<f64>,
}

impl<'a> Context<'a> {
    pub fn zoom(zoom: f64) -> Context<'static> {
        Context {
            zoom,
            feature: None,
            input: None,
        }
    }

    pub fn feature(zoom: f64, feature: &'a dyn FeatureView) -> Context<'a> {
        Context {
            zoom,
            feature: Some(feature),
            input: None,
        }
    }
}

/// The type a property's value is coerced to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Any,
    Number,
    String,
    Bool,
    Color,
    Array,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Interpolation {
    Linear,
    Exponential(f64),
    CubicBezier([f64; 4]),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Compare {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Debug)]
enum Expr {
    Literal(Value),
    Get(Arc<str>),
    GetFrom(Box<Expr>, Box<Expr>),
    GetDynamic(Box<Expr>),
    Has(Box<Expr>),
    FeatureState(Box<Expr>),
    Properties,
    Id,
    GeometryType,
    Zoom,
    Input,
    Interpolate {
        how: Interpolation,
        input: Box<Expr>,
        stops: Vec<(f64, Expr)>,
    },
    Step {
        input: Box<Expr>,
        first: Box<Expr>,
        stops: Vec<(f64, Expr)>,
    },
    Match {
        input: Box<Expr>,
        arms: Vec<(Vec<Value>, Expr)>,
        otherwise: Box<Expr>,
    },
    Case {
        arms: Vec<(Expr, Expr)>,
        otherwise: Box<Expr>,
    },
    Coalesce(Vec<Expr>),
    Compare(Compare, Box<Expr>, Box<Expr>),
    All(Vec<Expr>),
    Any(Vec<Expr>),
    Not(Box<Expr>),
    In(Box<Expr>, Box<Expr>),
    Math(Arc<str>, Vec<Expr>),
    Concat(Vec<Expr>),
    Call(Arc<str>, Vec<Expr>),
    Let(Vec<(Arc<str>, Expr)>, Box<Expr>),
    Var(Arc<str>),
    ArrayOf(Vec<Expr>),
}

const OPERATORS: &[&str] = &[
    "literal",
    "get",
    "has",
    "!has",
    "feature-state",
    "properties",
    "id",
    "geometry-type",
    "zoom",
    "heatmap-density",
    "line-progress",
    "elevation",
    "interpolate",
    "interpolate-hcl",
    "interpolate-lab",
    "step",
    "match",
    "case",
    "coalesce",
    "==",
    "!=",
    "<",
    "<=",
    ">",
    ">=",
    "all",
    "any",
    "none",
    "!",
    "in",
    "!in",
    "+",
    "-",
    "*",
    "/",
    "%",
    "^",
    "abs",
    "ceil",
    "floor",
    "round",
    "sqrt",
    "ln",
    "log10",
    "log2",
    "min",
    "max",
    "sin",
    "cos",
    "tan",
    "asin",
    "acos",
    "atan",
    "e",
    "pi",
    "ln2",
    "concat",
    "upcase",
    "downcase",
    "to-string",
    "to-number",
    "to-boolean",
    "to-color",
    "to-rgba",
    "rgb",
    "rgba",
    "at",
    "length",
    "slice",
    "index-of",
    "let",
    "var",
    "typeof",
    "format",
    "number-format",
    "image",
    "string",
    "number",
    "boolean",
    "array",
    "object",
    "collator",
    "resolved-locale",
    "is-supported-script",
    "within",
    "distance",
    "accumulated",
    "global-state",
    "split",
    "join",
    "config",
];

pub(crate) fn is_expression(json: &Json) -> bool {
    match json {
        Json::Array(items) => items
            .first()
            .and_then(Json::as_str)
            .is_some_and(|op| OPERATORS.contains(&op)),
        _ => false,
    }
}

/// A compiled property value with what it depends on.
#[derive(Clone, Debug)]
pub(crate) struct Property {
    expr: Expr,
    kind: Kind,
    pub feature_dependent: bool,
}

impl Property {
    pub fn constant(value: Value) -> Property {
        Property {
            expr: Expr::Literal(value),
            kind: Kind::Any,
            feature_dependent: false,
        }
    }

    pub fn parse(json: &Json, kind: Kind) -> Property {
        let expr = if is_expression(json) {
            parse(json)
        } else if let Json::Object(object) = json {
            if object.contains_key("stops") {
                parse_legacy_function(object, kind)
            } else {
                Expr::Literal(Value::from_json(json))
            }
        } else {
            Expr::Literal(literal(json, kind))
        };
        let (_, feature_dependent) = dependencies(&expr);
        Property {
            expr,
            kind,
            feature_dependent,
        }
    }

    pub fn filter(json: &Json) -> Property {
        let expr = if is_legacy_filter(json) {
            legacy_filter(json)
        } else {
            parse(json)
        };
        let (_, feature_dependent) = dependencies(&expr);
        Property {
            expr,
            kind: Kind::Bool,
            feature_dependent,
        }
    }

    pub fn eval(&self, ctx: &Context) -> Value {
        let mut scope = Vec::new();
        coerce(eval(&self.expr, ctx, &mut scope), self.kind)
    }

    pub fn number(&self, ctx: &Context, fallback: f64) -> f64 {
        self.eval(ctx).as_f64().unwrap_or(fallback)
    }

    pub fn color(&self, ctx: &Context, fallback: Rgba) -> Rgba {
        match self.eval(ctx) {
            Value::Color(color) => color,
            _ => fallback,
        }
    }

    pub fn string(&self, ctx: &Context) -> Option<String> {
        match self.eval(ctx) {
            Value::Null => None,
            value => Some(value.to_text()),
        }
    }

    pub fn bool(&self, ctx: &Context) -> bool {
        self.eval(ctx).truthy()
    }

    pub fn numbers(&self, ctx: &Context) -> Vec<f64> {
        match self.eval(ctx) {
            Value::Array(items) => items.iter().filter_map(Value::as_f64).collect(),
            Value::Number(n) => vec![n],
            _ => Vec::new(),
        }
    }

    pub fn strings(&self, ctx: &Context) -> Vec<String> {
        match self.eval(ctx) {
            Value::Array(items) => items.iter().map(Value::to_text).collect(),
            Value::Null => Vec::new(),
            other => vec![other.to_text()],
        }
    }
}

fn literal(json: &Json, kind: Kind) -> Value {
    coerce(Value::from_json(json), kind)
}

fn coerce(value: Value, kind: Kind) -> Value {
    match (kind, value) {
        (Kind::Color, Value::String(text)) => color::parse(&text).map_or(Value::Null, Value::Color),
        (Kind::Number, Value::String(text)) => text.parse().map_or(Value::Null, Value::Number),
        (_, value) => value,
    }
}

fn dependencies(expr: &Expr) -> (bool, bool) {
    let mut zoom = false;
    let mut feature = false;
    visit(expr, &mut |e| match e {
        Expr::Zoom => zoom = true,
        Expr::Get(_)
        | Expr::GetFrom(..)
        | Expr::GetDynamic(_)
        | Expr::Has(_)
        | Expr::FeatureState(_)
        | Expr::Properties
        | Expr::Id
        | Expr::GeometryType => feature = true,
        _ => {}
    });
    (zoom, feature)
}

fn visit(expr: &Expr, f: &mut dyn FnMut(&Expr)) {
    f(expr);
    let mut each = |items: &[Expr]| items.iter().for_each(|e| visit(e, f));
    match expr {
        Expr::GetFrom(a, b) | Expr::Compare(_, a, b) | Expr::In(a, b) => {
            visit(a, f);
            visit(b, f);
        }
        Expr::GetDynamic(a) | Expr::Has(a) | Expr::FeatureState(a) | Expr::Not(a) => visit(a, f),
        Expr::Interpolate { input, stops, .. } => {
            visit(input, f);
            stops.iter().for_each(|(_, e)| visit(e, f));
        }
        Expr::Step {
            input,
            first,
            stops,
        } => {
            visit(input, f);
            visit(first, f);
            stops.iter().for_each(|(_, e)| visit(e, f));
        }
        Expr::Match {
            input,
            arms,
            otherwise,
        } => {
            visit(input, f);
            arms.iter().for_each(|(_, e)| visit(e, f));
            visit(otherwise, f);
        }
        Expr::Case { arms, otherwise } => {
            arms.iter().for_each(|(c, e)| {
                visit(c, f);
                visit(e, f);
            });
            visit(otherwise, f);
        }
        Expr::Let(bindings, body) => {
            bindings.iter().for_each(|(_, e)| visit(e, f));
            visit(body, f);
        }
        Expr::Coalesce(items)
        | Expr::All(items)
        | Expr::Any(items)
        | Expr::Math(_, items)
        | Expr::Concat(items)
        | Expr::Call(_, items)
        | Expr::ArrayOf(items) => each(items),
        _ => {}
    }
}

fn parse(json: &Json) -> Expr {
    let Json::Array(items) = json else {
        return Expr::Literal(Value::from_json(json));
    };
    let Some(op) = items.first().and_then(Json::as_str) else {
        return Expr::ArrayOf(items.iter().map(parse).collect());
    };
    let args = &items[1..];
    let arg = |i: usize| args.get(i).map_or(Expr::Literal(Value::Null), parse);
    let all = || args.iter().map(parse).collect::<Vec<_>>();
    match op {
        "literal" => Expr::Literal(args.first().map_or(Value::Null, Value::from_json)),
        "get" => match (args.first(), args.len()) {
            (Some(Json::String(key)), 1) => Expr::Get(key.as_str().into()),
            (_, 1) => Expr::GetDynamic(Box::new(arg(0))),
            _ => Expr::GetFrom(Box::new(arg(0)), Box::new(arg(1))),
        },
        "has" => Expr::Has(Box::new(arg(0))),
        "!has" => Expr::Not(Box::new(Expr::Has(Box::new(arg(0))))),
        "feature-state" => Expr::FeatureState(Box::new(arg(0))),
        "properties" => Expr::Properties,
        "id" => Expr::Id,
        "geometry-type" => Expr::GeometryType,
        "zoom" => Expr::Zoom,
        "heatmap-density" | "line-progress" | "elevation" => Expr::Input,
        "interpolate" | "interpolate-hcl" | "interpolate-lab" => {
            let how = match args.first() {
                Some(Json::Array(kind)) => match kind.first().and_then(Json::as_str) {
                    Some("exponential") => Interpolation::Exponential(
                        kind.get(1).and_then(Json::as_f64).unwrap_or(1.0),
                    ),
                    Some("cubic-bezier") => {
                        let n = |i: usize| kind.get(i).and_then(Json::as_f64).unwrap_or(0.0);
                        Interpolation::CubicBezier([n(1), n(2), n(3), n(4)])
                    }
                    _ => Interpolation::Linear,
                },
                _ => Interpolation::Linear,
            };
            let stops = args
                .get(2..)
                .unwrap_or_default()
                .chunks_exact(2)
                .map(|pair| (pair[0].as_f64().unwrap_or(0.0), parse(&pair[1])))
                .collect();
            Expr::Interpolate {
                how,
                input: Box::new(arg(1)),
                stops,
            }
        }
        "step" => {
            let stops = args
                .get(2..)
                .unwrap_or_default()
                .chunks_exact(2)
                .map(|pair| (pair[0].as_f64().unwrap_or(0.0), parse(&pair[1])))
                .collect();
            Expr::Step {
                input: Box::new(arg(0)),
                first: Box::new(arg(1)),
                stops,
            }
        }
        "match" => {
            let rest = args.get(1..).unwrap_or_default();
            let (pairs, otherwise) = if rest.len() % 2 == 1 {
                (&rest[..rest.len() - 1], parse(&rest[rest.len() - 1]))
            } else {
                (rest, Expr::Literal(Value::Null))
            };
            let arms = pairs
                .chunks_exact(2)
                .map(|pair| {
                    let labels = match &pair[0] {
                        Json::Array(labels) => labels.iter().map(Value::from_json).collect(),
                        label => vec![Value::from_json(label)],
                    };
                    (labels, parse(&pair[1]))
                })
                .collect();
            Expr::Match {
                input: Box::new(arg(0)),
                arms,
                otherwise: Box::new(otherwise),
            }
        }
        "case" => {
            let (pairs, otherwise) = if args.len() % 2 == 1 {
                (&args[..args.len() - 1], parse(&args[args.len() - 1]))
            } else {
                (args, Expr::Literal(Value::Null))
            };
            Expr::Case {
                arms: pairs
                    .chunks_exact(2)
                    .map(|pair| (parse(&pair[0]), parse(&pair[1])))
                    .collect(),
                otherwise: Box::new(otherwise),
            }
        }
        "coalesce" => Expr::Coalesce(all()),
        "==" | "!=" | "<" | "<=" | ">" | ">=" => {
            let compare = match op {
                "==" => Compare::Eq,
                "!=" => Compare::Ne,
                "<" => Compare::Lt,
                "<=" => Compare::Le,
                ">" => Compare::Gt,
                _ => Compare::Ge,
            };
            Expr::Compare(compare, Box::new(arg(0)), Box::new(arg(1)))
        }
        "all" => Expr::All(all()),
        "any" => Expr::Any(all()),
        "none" => Expr::Not(Box::new(Expr::Any(all()))),
        "!" => Expr::Not(Box::new(arg(0))),
        "in" => Expr::In(Box::new(arg(0)), Box::new(arg(1))),
        "!in" => Expr::Not(Box::new(Expr::In(Box::new(arg(0)), Box::new(arg(1))))),
        "let" => {
            let mut bindings = Vec::new();
            let body = args.last().map_or(Expr::Literal(Value::Null), parse);
            for pair in args[..args.len().saturating_sub(1)].chunks_exact(2) {
                if let Some(name) = pair[0].as_str() {
                    bindings.push((name.into(), parse(&pair[1])));
                }
            }
            Expr::Let(bindings, Box::new(body))
        }
        "var" => Expr::Var(args.first().and_then(Json::as_str).unwrap_or("").into()),
        "concat" => Expr::Concat(all()),
        "+" | "-" | "*" | "/" | "%" | "^" | "abs" | "ceil" | "floor" | "round" | "sqrt" | "ln"
        | "log10" | "log2" | "min" | "max" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan"
        | "e" | "pi" | "ln2" => Expr::Math(op.into(), all()),
        _ => Expr::Call(op.into(), all()),
    }
}

fn parse_legacy_function(object: &serde_json::Map<String, Json>, kind: Kind) -> Expr {
    let stops: Vec<(Json, Json)> = object
        .get("stops")
        .and_then(Json::as_array)
        .map(|stops| {
            stops
                .iter()
                .filter_map(|stop| match stop {
                    Json::Array(pair) if pair.len() == 2 => {
                        Some((pair[0].clone(), pair[1].clone()))
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let base = object.get("base").and_then(Json::as_f64).unwrap_or(1.0);
    let property = object.get("property").and_then(Json::as_str);
    let default = object
        .get("default")
        .map_or(Expr::Literal(Value::Null), |d| {
            Expr::Literal(literal(d, kind))
        });
    let input = match property {
        Some(key) => Expr::Get(key.into()),
        None => Expr::Zoom,
    };
    let zoom_and_property = stops.first().is_some_and(|(input, _)| input.is_object());
    let default_type = match kind {
        Kind::Number | Kind::Color
            if property.is_none() || stops.first().is_some_and(|(i, _)| i.is_number()) =>
        {
            "exponential"
        }
        Kind::Number | Kind::Color => "categorical",
        _ => "interval",
    };
    let kind_name = object
        .get("type")
        .and_then(Json::as_str)
        .unwrap_or(default_type);
    if zoom_and_property {
        // A zoom-and-property function: interpolate by zoom over per-zoom property functions.
        let mut by_zoom: Vec<(f64, Vec<(Json, Json)>)> = Vec::new();
        for (input, output) in &stops {
            let zoom = input.get("zoom").and_then(Json::as_f64).unwrap_or(0.0);
            let value = input.get("value").cloned().unwrap_or(Json::Null);
            match by_zoom.last_mut() {
                Some((z, list)) if *z == zoom => list.push((value, output.clone())),
                _ => by_zoom.push((zoom, vec![(value, output.clone())])),
            }
        }
        let inner = by_zoom
            .into_iter()
            .map(|(zoom, stops)| {
                let mut inner = object.clone();
                inner.insert(
                    "stops".into(),
                    Json::Array(
                        stops
                            .into_iter()
                            .map(|(i, o)| Json::Array(vec![i, o]))
                            .collect(),
                    ),
                );
                (zoom, parse_legacy_function(&inner, kind))
            })
            .collect();
        return Expr::Interpolate {
            how: Interpolation::Exponential(base),
            input: Box::new(Expr::Zoom),
            stops: inner,
        };
    }
    let outputs: Vec<(Json, Expr)> = stops
        .iter()
        .map(|(i, o)| (i.clone(), Expr::Literal(literal(o, kind))))
        .collect();
    let body = match kind_name {
        "identity" => {
            return Expr::Coalesce(vec![input, default]);
        }
        "categorical" => Expr::Match {
            input: Box::new(input),
            arms: outputs
                .into_iter()
                .map(|(i, o)| (vec![Value::from_json(&i)], o))
                .collect(),
            otherwise: Box::new(default.clone()),
        },
        "interval" => {
            let mut stops: Vec<(f64, Expr)> = outputs
                .into_iter()
                .map(|(i, o)| (i.as_f64().unwrap_or(0.0), o))
                .collect();
            if stops.is_empty() {
                return default;
            }
            let (_, first) = stops.remove(0);
            Expr::Step {
                input: Box::new(input),
                first: Box::new(first),
                stops,
            }
        }
        _ => Expr::Interpolate {
            how: if base == 1.0 {
                Interpolation::Linear
            } else {
                Interpolation::Exponential(base)
            },
            input: Box::new(input),
            stops: outputs
                .into_iter()
                .map(|(i, o)| (i.as_f64().unwrap_or(0.0), o))
                .collect(),
        },
    };
    if property.is_some() {
        Expr::Coalesce(vec![body, default])
    } else {
        body
    }
}

fn is_legacy_filter(json: &Json) -> bool {
    let Json::Array(items) = json else {
        return false;
    };
    let Some(op) = items.first().and_then(Json::as_str) else {
        return false;
    };
    match op {
        "==" | "!=" | ">" | ">=" | "<" | "<=" => {
            items.len() == 3 && items[1].is_string() && !items[2].is_array()
        }
        "in" | "!in" => {
            items.len() >= 2 && items[1].is_string() && (items.len() != 3 || !items[2].is_array())
        }
        "!has" | "has" => items.len() == 2 && items[1].is_string(),
        "all" | "any" | "none" => items.len() > 1 && items[1..].iter().all(is_legacy_filter),
        _ => false,
    }
}

fn legacy_key(key: &str) -> Expr {
    match key {
        "$type" => Expr::GeometryType,
        "$id" => Expr::Id,
        key => Expr::Get(key.into()),
    }
}

fn legacy_filter(json: &Json) -> Expr {
    let Json::Array(items) = json else {
        return Expr::Literal(Value::Bool(true));
    };
    let op = items.first().and_then(Json::as_str).unwrap_or("");
    let key = items.get(1).and_then(Json::as_str).unwrap_or("");
    let value = |i: usize| Expr::Literal(items.get(i).map_or(Value::Null, Value::from_json));
    match op {
        "==" | "!=" | ">" | ">=" | "<" | "<=" => {
            let compare = match op {
                "==" => Compare::Eq,
                "!=" => Compare::Ne,
                ">" => Compare::Gt,
                ">=" => Compare::Ge,
                "<" => Compare::Lt,
                _ => Compare::Le,
            };
            Expr::Compare(compare, Box::new(legacy_key(key)), Box::new(value(2)))
        }
        "in" | "!in" => {
            let haystack = Expr::Literal(Value::Array(
                items[2..].iter().map(Value::from_json).collect(),
            ));
            let found = Expr::In(Box::new(legacy_key(key)), Box::new(haystack));
            if op == "in" {
                found
            } else {
                Expr::Not(Box::new(found))
            }
        }
        "has" => Expr::Has(Box::new(Expr::Literal(Value::String(key.into())))),
        "!has" => Expr::Not(Box::new(Expr::Has(Box::new(Expr::Literal(Value::String(
            key.into(),
        )))))),
        "all" | "any" | "none" => {
            let parts: Vec<Expr> = items[1..].iter().map(legacy_filter).collect();
            match op {
                "all" => Expr::All(parts),
                "any" => Expr::Any(parts),
                _ => Expr::Not(Box::new(Expr::Any(parts))),
            }
        }
        _ => parse(json),
    }
}

type Scope = Vec<(Arc<str>, Value)>;

fn num(value: &Value) -> f64 {
    match value {
        Value::Number(n) => *n,
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::String(s) => s.parse().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

fn eval(expr: &Expr, ctx: &Context, scope: &mut Scope) -> Value {
    match expr {
        Expr::Literal(value) => value.clone(),
        Expr::Get(key) => ctx
            .feature
            .and_then(|f| f.property(key))
            .unwrap_or(Value::Null),
        Expr::GetDynamic(key) => {
            let key = eval(key, ctx, scope).to_text();
            ctx.feature
                .and_then(|f| f.property(&key))
                .unwrap_or(Value::Null)
        }
        Expr::GetFrom(key, object) => {
            let key = eval(key, ctx, scope).to_text();
            match eval(object, ctx, scope) {
                Value::Object(map) => map.get(&key).map_or(Value::Null, Value::from_json),
                _ => Value::Null,
            }
        }
        Expr::Has(key) => {
            let key = eval(key, ctx, scope).to_text();
            Value::Bool(ctx.feature.and_then(|f| f.property(&key)).is_some())
        }
        Expr::FeatureState(key) => {
            let key = eval(key, ctx, scope).to_text();
            ctx.feature
                .and_then(|f| f.state(&key))
                .unwrap_or(Value::Null)
        }
        Expr::Properties => ctx.feature.map_or(Value::Null, |f| f.properties()),
        Expr::Id => ctx.feature.and_then(|f| f.id()).unwrap_or(Value::Null),
        Expr::GeometryType => Value::String(
            match ctx.feature.map(|f| f.kind()) {
                Some(GeometryKind::Point) => "Point",
                Some(GeometryKind::Line) => "LineString",
                Some(GeometryKind::Polygon) => "Polygon",
                None => "",
            }
            .into(),
        ),
        Expr::Zoom => Value::Number(ctx.zoom),
        Expr::Input => Value::Number(ctx.input.unwrap_or(0.0)),
        Expr::Interpolate { how, input, stops } => {
            let x = num(&eval(input, ctx, scope));
            interpolate(*how, x, stops, ctx, scope)
        }
        Expr::Step {
            input,
            first,
            stops,
        } => {
            let x = num(&eval(input, ctx, scope));
            let mut chosen: &Expr = first;
            for (stop, output) in stops {
                if x >= *stop {
                    chosen = output;
                } else {
                    break;
                }
            }
            eval(chosen, ctx, scope)
        }
        Expr::Match {
            input,
            arms,
            otherwise,
        } => {
            let value = eval(input, ctx, scope);
            for (labels, output) in arms {
                if labels.iter().any(|label| loose_eq(label, &value)) {
                    return eval(output, ctx, scope);
                }
            }
            eval(otherwise, ctx, scope)
        }
        Expr::Case { arms, otherwise } => {
            for (condition, output) in arms {
                if eval(condition, ctx, scope).truthy() {
                    return eval(output, ctx, scope);
                }
            }
            eval(otherwise, ctx, scope)
        }
        Expr::Coalesce(items) => items
            .iter()
            .map(|e| eval(e, ctx, scope))
            .find(|v| *v != Value::Null)
            .unwrap_or(Value::Null),
        Expr::Compare(op, a, b) => {
            let a = eval(a, ctx, scope);
            let b = eval(b, ctx, scope);
            Value::Bool(compare(*op, &a, &b))
        }
        Expr::All(items) => Value::Bool(items.iter().all(|e| eval(e, ctx, scope).truthy())),
        Expr::Any(items) => Value::Bool(items.iter().any(|e| eval(e, ctx, scope).truthy())),
        Expr::Not(e) => Value::Bool(!eval(e, ctx, scope).truthy()),
        Expr::In(needle, haystack) => {
            let needle = eval(needle, ctx, scope);
            Value::Bool(match eval(haystack, ctx, scope) {
                Value::Array(items) => items.iter().any(|item| loose_eq(item, &needle)),
                Value::String(text) => text.contains(needle.to_text().as_str()),
                _ => false,
            })
        }
        Expr::Math(op, args) => {
            let values: Vec<f64> = args.iter().map(|e| num(&eval(e, ctx, scope))).collect();
            Value::Number(math(op, &values))
        }
        Expr::Concat(items) => {
            let mut out = String::new();
            for item in items {
                out.push_str(&eval(item, ctx, scope).to_text());
            }
            Value::String(out.into())
        }
        Expr::Let(bindings, body) => {
            let depth = scope.len();
            for (name, value) in bindings {
                let value = eval(value, ctx, scope);
                scope.push((name.clone(), value));
            }
            let result = eval(body, ctx, scope);
            scope.truncate(depth);
            result
        }
        Expr::Var(name) => scope
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map_or(Value::Null, |(_, v)| v.clone()),
        Expr::ArrayOf(items) => Value::Array(items.iter().map(|e| eval(e, ctx, scope)).collect()),
        Expr::Call(op, args) => call(op, args, ctx, scope),
    }
}

fn loose_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x == y,
        _ => a == b,
    }
}

fn compare(op: Compare, a: &Value, b: &Value) -> bool {
    use std::cmp::Ordering;
    let ordering = match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.partial_cmp(y),
        (Value::String(x), Value::String(y)) => Some(x.cmp(y)),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        _ => None,
    };
    match op {
        Compare::Eq => loose_eq(a, b),
        Compare::Ne => !loose_eq(a, b),
        Compare::Lt => ordering == Some(Ordering::Less),
        Compare::Le => matches!(ordering, Some(Ordering::Less | Ordering::Equal)),
        Compare::Gt => ordering == Some(Ordering::Greater),
        Compare::Ge => matches!(ordering, Some(Ordering::Greater | Ordering::Equal)),
    }
}

fn math(op: &str, v: &[f64]) -> f64 {
    let a = v.first().copied().unwrap_or(f64::NAN);
    let b = v.get(1).copied().unwrap_or(f64::NAN);
    match op {
        "+" => v.iter().sum(),
        "*" => v.iter().product(),
        "-" if v.len() == 1 => -a,
        "-" => a - b,
        "/" => a / b,
        "%" => a % b,
        "^" => a.powf(b),
        "abs" => a.abs(),
        "ceil" => a.ceil(),
        "floor" => a.floor(),
        "round" => {
            if a < 0.0 {
                -(-a).round()
            } else {
                a.round()
            }
        }
        "sqrt" => a.sqrt(),
        "ln" => a.ln(),
        "log10" => a.log10(),
        "log2" => a.log2(),
        "min" => v.iter().copied().fold(f64::INFINITY, f64::min),
        "max" => v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        "sin" => a.sin(),
        "cos" => a.cos(),
        "tan" => a.tan(),
        "asin" => a.asin(),
        "acos" => a.acos(),
        "atan" => a.atan(),
        "e" => std::f64::consts::E,
        "pi" => std::f64::consts::PI,
        "ln2" => std::f64::consts::LN_2,
        _ => f64::NAN,
    }
}

fn call(op: &str, args: &[Expr], ctx: &Context, scope: &mut Scope) -> Value {
    let arg =
        |i: usize, scope: &mut Scope| args.get(i).map_or(Value::Null, |e| eval(e, ctx, scope));
    match op {
        "upcase" => Value::String(arg(0, scope).to_text().to_uppercase().into()),
        "downcase" => Value::String(arg(0, scope).to_text().to_lowercase().into()),
        "to-string" => Value::String(arg(0, scope).to_text().into()),
        "to-number" => {
            for i in 0..args.len().max(1) {
                let value = arg(i, scope);
                let n = match &value {
                    Value::Null => Some(0.0),
                    Value::Bool(b) => Some(f64::from(u8::from(*b))),
                    Value::Number(n) => Some(*n),
                    Value::String(s) => s.trim().parse().ok(),
                    _ => None,
                };
                if let Some(n) = n {
                    return Value::Number(n);
                }
            }
            Value::Null
        }
        "to-boolean" => Value::Bool(arg(0, scope).truthy()),
        "to-color" => {
            for i in 0..args.len() {
                match arg(i, scope) {
                    Value::Color(c) => return Value::Color(c),
                    Value::String(s) => {
                        if let Some(c) = color::parse(&s) {
                            return Value::Color(c);
                        }
                    }
                    Value::Array(items) if items.len() >= 3 => {
                        let n = |i: usize| items.get(i).map_or(1.0, num) as f32;
                        return Value::Color(Rgba([
                            n(0) / 255.0,
                            n(1) / 255.0,
                            n(2) / 255.0,
                            if items.len() > 3 { n(3) } else { 1.0 },
                        ]));
                    }
                    _ => {}
                }
            }
            Value::Null
        }
        "to-rgba" => match coerce(arg(0, scope), Kind::Color) {
            Value::Color(Rgba([r, g, b, a])) => Value::Array(
                vec![
                    Value::Number(f64::from(r * 255.0)),
                    Value::Number(f64::from(g * 255.0)),
                    Value::Number(f64::from(b * 255.0)),
                    Value::Number(f64::from(a)),
                ]
                .into(),
            ),
            _ => Value::Null,
        },
        "rgb" | "rgba" => {
            let n = |v: Value| num(&v) as f32;
            let r = n(arg(0, scope));
            let g = n(arg(1, scope));
            let b = n(arg(2, scope));
            let a = if op == "rgba" { n(arg(3, scope)) } else { 1.0 };
            Value::Color(Rgba([r / 255.0, g / 255.0, b / 255.0, a]))
        }
        "at" => {
            let index = num(&arg(0, scope));
            match arg(1, scope) {
                Value::Array(items) => items.get(index as usize).cloned().unwrap_or(Value::Null),
                _ => Value::Null,
            }
        }
        "length" => match arg(0, scope) {
            Value::Array(items) => Value::Number(items.len() as f64),
            Value::String(s) => Value::Number(s.chars().count() as f64),
            _ => Value::Null,
        },
        "slice" => {
            let input = arg(0, scope);
            let start = num(&arg(1, scope));
            let end = (args.len() > 2).then(|| num(&arg(2, scope)));
            let clamp = |i: f64, len: usize| -> usize {
                let i = if i < 0.0 { len as f64 + i } else { i };
                i.clamp(0.0, len as f64) as usize
            };
            match input {
                Value::String(s) => {
                    let chars: Vec<char> = s.chars().collect();
                    let a = clamp(start, chars.len());
                    let b = end.map_or(chars.len(), |e| clamp(e, chars.len()));
                    Value::String(chars[a..b.max(a)].iter().collect::<String>().into())
                }
                Value::Array(items) => {
                    let a = clamp(start, items.len());
                    let b = end.map_or(items.len(), |e| clamp(e, items.len()));
                    Value::Array(items[a..b.max(a)].to_vec().into())
                }
                _ => Value::Null,
            }
        }
        "index-of" => {
            let needle = arg(0, scope);
            match arg(1, scope) {
                Value::Array(items) => Value::Number(
                    items
                        .iter()
                        .position(|i| loose_eq(i, &needle))
                        .map_or(-1.0, |p| p as f64),
                ),
                Value::String(s) => Value::Number(
                    s.find(needle.to_text().as_str())
                        .map_or(-1.0, |byte| s[..byte].chars().count() as f64),
                ),
                _ => Value::Number(-1.0),
            }
        }
        "split" => {
            let text = arg(0, scope).to_text();
            let separator = arg(1, scope).to_text();
            Value::Array(
                text.split(separator.as_str())
                    .map(|p| Value::String(p.into()))
                    .collect(),
            )
        }
        "join" => match arg(0, scope) {
            Value::Array(items) => {
                let separator = arg(1, scope).to_text();
                Value::String(
                    items
                        .iter()
                        .map(Value::to_text)
                        .collect::<Vec<_>>()
                        .join(&separator)
                        .into(),
                )
            }
            _ => Value::Null,
        },
        "typeof" => Value::String(arg(0, scope).type_name().into()),
        // Sections keep only their text; per-section styles are not drawn.
        "format" => {
            let mut out = String::new();
            for (i, e) in args.iter().enumerate() {
                if matches!(e, Expr::Literal(Value::Object(_))) && i > 0 {
                    continue;
                }
                out.push_str(&eval(e, ctx, scope).to_text());
            }
            Value::String(out.into())
        }
        "number-format" => {
            let n = num(&arg(0, scope));
            let options = arg(1, scope);
            let (min, max) = match &options {
                Value::Object(map) => (
                    map.get("min-fraction-digits").and_then(Json::as_u64),
                    map.get("max-fraction-digits").and_then(Json::as_u64),
                ),
                _ => (None, None),
            };
            let digits = max.or(min).unwrap_or(3) as usize;
            let mut text = format!("{n:.digits$}");
            if text.contains('.') && min.is_none_or(|m| (m as usize) < digits) {
                let keep = min.unwrap_or(0) as usize;
                while text.ends_with('0') && text.split('.').nth(1).is_some_and(|f| f.len() > keep)
                {
                    text.pop();
                }
                if text.ends_with('.') {
                    text.pop();
                }
            }
            Value::String(text.into())
        }
        "image" => Value::String(arg(0, scope).to_text().into()),
        "string" | "number" | "boolean" | "array" | "object" => {
            for i in 0..args.len() {
                let value = arg(i, scope);
                let fits = matches!(
                    (op, &value),
                    ("string", Value::String(_))
                        | ("number", Value::Number(_))
                        | ("boolean", Value::Bool(_))
                        | ("object", Value::Object(_))
                        | ("array", Value::Array(_))
                );
                if fits {
                    return value;
                }
            }
            if op == "array" {
                args.last().map_or(Value::Null, |e| eval(e, ctx, scope))
            } else {
                Value::Null
            }
        }
        "resolved-locale" => Value::String("en".into()),
        "is-supported-script" => Value::Bool(true),
        "collator" => Value::Null,
        _ => Value::Null,
    }
}

fn interpolate(
    how: Interpolation,
    x: f64,
    stops: &[(f64, Expr)],
    ctx: &Context,
    scope: &mut Scope,
) -> Value {
    let Some(first) = stops.first() else {
        return Value::Null;
    };
    if stops.len() == 1 || x <= first.0 || x.is_nan() {
        return eval(&first.1, ctx, scope);
    }
    let last = stops.last().unwrap();
    if x >= last.0 {
        return eval(&last.1, ctx, scope);
    }
    let index = stops
        .iter()
        .position(|(s, _)| *s > x)
        .unwrap_or(stops.len() - 1);
    let (lo, hi) = (&stops[index - 1], &stops[index]);
    let t = interpolation_factor(how, x, lo.0, hi.0);
    let a = eval(&lo.1, ctx, scope);
    let b = eval(&hi.1, ctx, scope);
    lerp_value(&a, &b, t)
}

fn interpolation_factor(how: Interpolation, x: f64, lo: f64, hi: f64) -> f64 {
    let span = hi - lo;
    if span == 0.0 {
        return 0.0;
    }
    let progress = x - lo;
    match how {
        Interpolation::Linear => progress / span,
        Interpolation::Exponential(base) => {
            if (base - 1.0).abs() < 1e-9 {
                progress / span
            } else {
                (base.powf(progress) - 1.0) / (base.powf(span) - 1.0)
            }
        }
        Interpolation::CubicBezier([x1, y1, x2, y2]) => bezier(progress / span, x1, y1, x2, y2),
    }
}

fn bezier(t: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;
    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;
    let sample_x = |s: f64| ((ax * s + bx) * s + cx) * s;
    let mut s = t;
    for _ in 0..8 {
        let x = sample_x(s) - t;
        let dx = (3.0 * ax * s + 2.0 * bx) * s + cx;
        if x.abs() < 1e-6 || dx.abs() < 1e-6 {
            break;
        }
        s -= x / dx;
    }
    ((ay * s + by) * s + cy) * s
}

pub(crate) fn lerp_value(a: &Value, b: &Value, t: f64) -> Value {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => Value::Number(x + (y - x) * t),
        (Value::Color(x), Value::Color(y)) => Value::Color(x.lerp(*y, t as f32)),
        (Value::String(x), Value::String(y)) => match (color::parse(x), color::parse(y)) {
            (Some(x), Some(y)) => Value::Color(x.lerp(y, t as f32)),
            _ => a.clone(),
        },
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => Value::Array(
            x.iter()
                .zip(y.iter())
                .map(|(x, y)| lerp_value(x, y, t))
                .collect(),
        ),
        _ => {
            if t < 1.0 {
                a.clone()
            } else {
                b.clone()
            }
        }
    }
}

/// A feature built from a plain property map.
#[cfg(test)]
pub(crate) struct MapFeature<'a> {
    pub kind: GeometryKind,
    pub id: Option<Value>,
    pub properties: &'a HashMap<Arc<str>, Value>,
}

#[cfg(test)]
impl FeatureView for MapFeature<'_> {
    fn property(&self, key: &str) -> Option<Value> {
        self.properties.get(key).cloned()
    }

    fn kind(&self) -> GeometryKind {
        self.kind
    }

    fn id(&self) -> Option<Value> {
        self.id.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn feature(props: &[(&str, Value)]) -> HashMap<Arc<str>, Value> {
        props
            .iter()
            .map(|(k, v)| ((*k).into(), v.clone()))
            .collect()
    }

    fn eval_with(json: Json, kind: Kind, zoom: f64, props: &HashMap<Arc<str>, Value>) -> Value {
        let feature = MapFeature {
            kind: GeometryKind::Polygon,
            id: Some(Value::Number(7.0)),
            properties: props,
        };
        Property::parse(&json, kind).eval(&Context::feature(zoom, &feature))
    }

    #[test]
    fn interpolates_by_zoom() {
        let props = feature(&[]);
        let width = json!([
            "interpolate",
            ["exponential", 1.2],
            ["zoom"],
            5,
            0.4,
            6,
            0.7,
            7,
            1.75
        ]);
        assert_eq!(
            eval_with(width.clone(), Kind::Number, 4.0, &props),
            Value::Number(0.4)
        );
        assert_eq!(
            eval_with(width.clone(), Kind::Number, 8.0, &props),
            Value::Number(1.75)
        );
        let mid = eval_with(width, Kind::Number, 5.5, &props)
            .as_f64()
            .unwrap();
        assert!(mid > 0.4 && mid < 0.55, "{mid}");
        let color = json!(["interpolate", ["linear"], ["zoom"], 0, "#000", 10, "#fff"]);
        let Value::Color(c) = eval_with(color, Kind::Color, 5.0, &props) else {
            panic!()
        };
        assert!((c.0[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn matches_and_filters() {
        let props = feature(&[
            ("class", Value::String("primary".into())),
            ("rank", Value::Number(3.0)),
        ]);
        let expr = json!(["match", ["get", "class"], ["primary", "trunk"], 1, 0]);
        assert_eq!(
            eval_with(expr, Kind::Number, 0.0, &props),
            Value::Number(1.0)
        );
        let filter = Property::filter(&json!([
            "all",
            ["==", "$type", "Polygon"],
            ["in", "class", "primary", "secondary"],
            ["<=", "rank", 4]
        ]));
        let view = MapFeature {
            kind: GeometryKind::Polygon,
            id: None,
            properties: &props,
        };
        assert!(filter.bool(&Context::feature(0.0, &view)));
        let filter = Property::filter(&json!(["!=", ["get", "class"], "primary"]));
        assert!(!filter.bool(&Context::feature(0.0, &view)));
        let filter = Property::filter(&json!(["!has", "name"]));
        assert!(filter.bool(&Context::feature(0.0, &view)));
    }

    #[test]
    fn legacy_functions() {
        let props = feature(&[]);
        let f = json!({"base": 1.4, "stops": [[14, 0], [20, 2]]});
        assert_eq!(
            eval_with(f.clone(), Kind::Number, 10.0, &props),
            Value::Number(0.0)
        );
        assert_eq!(eval_with(f, Kind::Number, 20.0, &props), Value::Number(2.0));
        let step = json!({"stops": [[5, "a"], [10, "b"]], "type": "interval"});
        assert_eq!(
            eval_with(step, Kind::String, 7.0, &props),
            Value::String("a".into())
        );
    }

    #[test]
    fn text_fields() {
        let props = feature(&[
            ("name", Value::String("東京".into())),
            ("name:en", Value::String("Tokyo".into())),
        ]);
        let expr = json!(["coalesce", ["get", "name:ja"], ["get", "name"]]);
        assert_eq!(
            eval_with(expr, Kind::String, 0.0, &props),
            Value::String("東京".into())
        );
        let expr = json!(["concat", ["get", "name:en"], "\n", ["get", "name"]]);
        assert_eq!(
            eval_with(expr, Kind::String, 0.0, &props),
            Value::String("Tokyo\n東京".into())
        );
        let expr = json!([
            "case",
            ["has", "name:en"],
            ["upcase", ["get", "name:en"]],
            ""
        ]);
        assert_eq!(
            eval_with(expr, Kind::String, 0.0, &props),
            Value::String("TOKYO".into())
        );
    }

    #[test]
    fn literals_are_not_expressions() {
        let props = feature(&[]);
        let fonts = json!(["Noto Sans Regular"]);
        assert!(!is_expression(&fonts));
        assert_eq!(
            eval_with(fonts, Kind::Array, 0.0, &props),
            Value::Array(vec![Value::String("Noto Sans Regular".into())].into())
        );
    }

    #[test]
    fn tracks_dependencies() {
        assert_eq!(dependencies(&parse(&json!(["get", "h"]))), (false, true));
        let zoom = json!(["interpolate", ["linear"], ["zoom"], 0, 1, 5, 2]);
        assert_eq!(dependencies(&parse(&zoom)), (true, false));
    }
}
