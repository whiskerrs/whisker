use super::*;

pub(super) fn materialize_custom_properties(
    specified: &SpecifiedStyle,
    inherited: &InheritedStyle,
) -> (SpecifiedStyle, BTreeMap<CustomPropertyName, StyleValue>) {
    let mut candidates = inherited.custom_properties.clone();
    let mut local_names = BTreeSet::new();
    for declaration in specified.resolved_custom() {
        local_names.insert(declaration.name().clone());
        candidates.insert(declaration.name().clone(), declaration.value().clone());
    }
    let cyclic = local_names
        .iter()
        .filter(|name| custom_property_reaches(name, name, &candidates, &mut BTreeSet::new()))
        .cloned()
        .collect::<BTreeSet<_>>();

    let mut cache: BTreeMap<CustomPropertyName, Option<StyleValue>> = BTreeMap::new();
    let names = candidates.keys().cloned().collect::<Vec<_>>();
    for name in names {
        let mut visiting = Vec::new();
        let _ = resolve_custom_name(&name, &candidates, &cyclic, &mut cache, &mut visiting);
    }
    let computed = cache
        .into_iter()
        .filter_map(|(name, value)| value.map(|value| (name, value)))
        .collect::<BTreeMap<_, _>>();

    let mut materialized = SpecifiedStyle::new();
    for declaration in specified.declarations() {
        let value = resolve_value_from_computed(declaration.value(), &computed);
        if let Some(value) = value {
            materialized = materialized.push(declaration.property(), value);
        }
    }
    (materialized, computed)
}

pub(super) fn custom_property_reaches(
    start: &CustomPropertyName,
    current: &CustomPropertyName,
    candidates: &BTreeMap<CustomPropertyName, StyleValue>,
    visited: &mut BTreeSet<CustomPropertyName>,
) -> bool {
    if !visited.insert(current.clone()) {
        return false;
    }
    let Some(value) = candidates.get(current) else {
        return false;
    };
    let mut references = Vec::new();
    collect_custom_references(value, &mut references);
    references
        .into_iter()
        .any(|next| next == start || custom_property_reaches(start, next, candidates, visited))
}

pub(super) fn collect_custom_references<'a>(
    value: &'a StyleValue,
    references: &mut Vec<&'a CustomPropertyName>,
) {
    if let StyleValue::Variable(reference) = value {
        collect_reference(reference, references);
        return;
    }
    crate::value_tree::visit_length_percentages(value, &mut |value| {
        collect_length_percentage_references(value, references);
    });
    crate::value_tree::visit_component_variables(value, &mut |reference| {
        collect_reference(reference, references);
    });
}

pub(super) fn collect_reference<'a>(
    reference: &'a CustomPropertyReference,
    references: &mut Vec<&'a CustomPropertyName>,
) {
    references.push(&reference.name);
    if let Some(fallback) = reference.fallback.as_deref() {
        collect_custom_references(fallback, references);
    }
}

pub(super) fn collect_length_percentage_references<'a>(
    value: &'a LengthPercentageValue,
    references: &mut Vec<&'a CustomPropertyName>,
) {
    if let LengthPercentageValue::Calc(expression) = value {
        collect_calc_references(expression, references);
    }
}

pub(super) fn collect_calc_references<'a>(
    expression: &'a CalcExpression,
    references: &mut Vec<&'a CustomPropertyName>,
) {
    match expression {
        CalcExpression::Variable(reference) => collect_reference(reference, references),
        CalcExpression::Value(value) => collect_length_percentage_references(value, references),
        CalcExpression::Add(left, right)
        | CalcExpression::Sub(left, right)
        | CalcExpression::Mul(left, right)
        | CalcExpression::Div(left, right) => {
            collect_calc_references(left, references);
            collect_calc_references(right, references);
        }
        CalcExpression::Number(_) => {}
    }
}

pub(super) fn resolve_custom_name(
    name: &CustomPropertyName,
    candidates: &BTreeMap<CustomPropertyName, StyleValue>,
    cyclic: &BTreeSet<CustomPropertyName>,
    cache: &mut BTreeMap<CustomPropertyName, Option<StyleValue>>,
    visiting: &mut Vec<CustomPropertyName>,
) -> Option<StyleValue> {
    if let Some(cached) = cache.get(name) {
        return cached.clone();
    }
    if cyclic.contains(name) || visiting.iter().any(|candidate| candidate == name) {
        cache.insert(name.clone(), None);
        return None;
    }
    visiting.push(name.clone());
    let resolved = candidates.get(name).and_then(|value| {
        resolve_value_from_candidates(value, candidates, cyclic, cache, visiting)
    });
    visiting.pop();
    cache.insert(name.clone(), resolved.clone());
    resolved
}

pub(super) fn resolve_reference_from_candidates(
    reference: &CustomPropertyReference,
    candidates: &BTreeMap<CustomPropertyName, StyleValue>,
    cyclic: &BTreeSet<CustomPropertyName>,
    cache: &mut BTreeMap<CustomPropertyName, Option<StyleValue>>,
    visiting: &mut Vec<CustomPropertyName>,
) -> Option<StyleValue> {
    resolve_custom_name(&reference.name, candidates, cyclic, cache, visiting).or_else(|| {
        reference.fallback.as_deref().and_then(|fallback| {
            resolve_value_from_candidates(fallback, candidates, cyclic, cache, visiting)
        })
    })
}

pub(super) fn resolve_reference_from_computed(
    reference: &CustomPropertyReference,
    computed: &BTreeMap<CustomPropertyName, StyleValue>,
) -> Option<StyleValue> {
    computed.get(&reference.name).cloned().or_else(|| {
        reference
            .fallback
            .as_deref()
            .and_then(|fallback| resolve_value_from_computed(fallback, computed))
    })
}

pub(super) fn resolve_value_from_candidates(
    value: &StyleValue,
    candidates: &BTreeMap<CustomPropertyName, StyleValue>,
    cyclic: &BTreeSet<CustomPropertyName>,
    cache: &mut BTreeMap<CustomPropertyName, Option<StyleValue>>,
    visiting: &mut Vec<CustomPropertyName>,
) -> Option<StyleValue> {
    match value {
        StyleValue::Variable(reference) => {
            resolve_reference_from_candidates(reference, candidates, cyclic, cache, visiting)
        }
        value => {
            let value =
                crate::value_tree::try_map_component_variables(value, &mut |reference, _kind| {
                    resolve_reference_from_candidates(
                        reference, candidates, cyclic, cache, visiting,
                    )
                })?;
            let mut resolve = |value: &LengthPercentageValue| {
                resolve_length_percentage_from_candidates(
                    value, candidates, cyclic, cache, visiting,
                )
            };
            map_nested_length_percentages(&value, &mut resolve)
        }
    }
}

pub(super) fn resolve_value_from_computed(
    value: &StyleValue,
    computed: &BTreeMap<CustomPropertyName, StyleValue>,
) -> Option<StyleValue> {
    match value {
        StyleValue::Variable(reference) => resolve_reference_from_computed(reference, computed),
        value => {
            let value =
                crate::value_tree::try_map_component_variables(value, &mut |reference, _kind| {
                    resolve_reference_from_computed(reference, computed)
                })?;
            let mut resolve = |value: &LengthPercentageValue| {
                resolve_length_percentage_from_computed(value, computed)
            };
            map_nested_length_percentages(&value, &mut resolve)
        }
    }
}

pub(super) fn map_nested_length_percentages(
    value: &StyleValue,
    resolve: &mut dyn FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
) -> Option<StyleValue> {
    crate::value_tree::try_map_length_percentages(value, resolve)
}

pub(super) fn resolve_length_percentage_from_candidates(
    value: &LengthPercentageValue,
    candidates: &BTreeMap<CustomPropertyName, StyleValue>,
    cyclic: &BTreeSet<CustomPropertyName>,
    cache: &mut BTreeMap<CustomPropertyName, Option<StyleValue>>,
    visiting: &mut Vec<CustomPropertyName>,
) -> Option<LengthPercentageValue> {
    resolve_length_percentage_with(value, &mut |reference| {
        resolve_reference_from_candidates(reference, candidates, cyclic, cache, visiting)
    })
}

pub(super) fn resolve_length_percentage_from_computed(
    value: &LengthPercentageValue,
    computed: &BTreeMap<CustomPropertyName, StyleValue>,
) -> Option<LengthPercentageValue> {
    resolve_length_percentage_with(value, &mut |reference| {
        resolve_reference_from_computed(reference, computed)
    })
}

pub(super) fn resolve_length_percentage_with(
    value: &LengthPercentageValue,
    resolve_reference: &mut dyn FnMut(&CustomPropertyReference) -> Option<StyleValue>,
) -> Option<LengthPercentageValue> {
    match value {
        LengthPercentageValue::Calc(expression) => Some(LengthPercentageValue::Calc(Box::new(
            resolve_calc_with(expression, resolve_reference)?,
        ))),
        value => Some(value.clone()),
    }
}

pub(super) fn resolve_calc_with(
    expression: &CalcExpression,
    resolve_reference: &mut dyn FnMut(&CustomPropertyReference) -> Option<StyleValue>,
) -> Option<CalcExpression> {
    match expression {
        CalcExpression::Variable(reference) => {
            resolve_reference(reference).and_then(style_value_to_calc)
        }
        CalcExpression::Value(value) => Some(CalcExpression::Value(Box::new(
            resolve_length_percentage_with(value, resolve_reference)?,
        ))),
        CalcExpression::Number(value) => Some(CalcExpression::Number(*value)),
        CalcExpression::Add(left, right) => Some(CalcExpression::Add(
            Box::new(resolve_calc_with(left, resolve_reference)?),
            Box::new(resolve_calc_with(right, resolve_reference)?),
        )),
        CalcExpression::Sub(left, right) => Some(CalcExpression::Sub(
            Box::new(resolve_calc_with(left, resolve_reference)?),
            Box::new(resolve_calc_with(right, resolve_reference)?),
        )),
        CalcExpression::Mul(left, right) => Some(CalcExpression::Mul(
            Box::new(resolve_calc_with(left, resolve_reference)?),
            Box::new(resolve_calc_with(right, resolve_reference)?),
        )),
        CalcExpression::Div(left, right) => Some(CalcExpression::Div(
            Box::new(resolve_calc_with(left, resolve_reference)?),
            Box::new(resolve_calc_with(right, resolve_reference)?),
        )),
    }
}

pub(super) fn style_value_to_calc(value: StyleValue) -> Option<CalcExpression> {
    match value {
        StyleValue::Number(value) => Some(CalcExpression::Number(value)),
        StyleValue::Length(value) => Some(CalcExpression::Value(Box::new(
            LengthPercentageValue::Length(value),
        ))),
        StyleValue::LengthPercentage(LengthPercentageValue::Calc(expression)) => Some(*expression),
        StyleValue::LengthPercentage(value) => Some(CalcExpression::Value(Box::new(value))),
        _ => None,
    }
}
