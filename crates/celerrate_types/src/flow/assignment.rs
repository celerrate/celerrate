//! Literals and assignment: interpolated strings, array literals
//! (shape versus list versus map), simple and compound assignment,
//! and assignment targets (locals, properties, array writes) — plus
//! the pure helpers they reduce to.

use super::*;

impl<'db> Walker<'db, '_, '_> {
    pub(super) fn string_parts(
        &mut self,
        parts: &[StringPart],
        environment: &mut Environment<'db>,
    ) {
        for part in parts {
            if let StringPart::Interpolation { expression } = part {
                self.expression(*expression, environment);
            }
        }
    }

    /// An array literal: a shape when every entry has a statically
    /// known key (or none — positional) and no spread; otherwise the
    /// general array of the joined keys and values.
    pub(super) fn array_literal(
        &mut self,
        entries: &[ArrayEntry],
        environment: &mut Environment<'db>,
    ) -> TypeId<'db> {
        let db = self.db();
        let mut fields: Vec<crate::representation::ShapeField<'db>> = Vec::new();
        let mut next_index: i64 = 0;
        let mut shape_holds = true;
        let mut joined_key: Option<TypeId<'db>> = None;
        let mut joined_value: Option<TypeId<'db>> = None;
        let mut is_list = true;
        for entry in entries {
            let ArrayEntry::Element {
                key,
                value,
                spread,
                by_reference: _,
            } = entry
            else {
                continue; // a destructuring hole never appears in a literal read
            };
            let key_type = key.map(|key| self.expression(key, environment));
            let value_type = self.expression(*value, environment);
            if *spread {
                shape_holds = false;
                is_list = false;
                let spread_key = value_type
                    .array_key(db)
                    .unwrap_or_else(|| TypeId::union(db, [TypeId::int(db), TypeId::string(db)]));
                let spread_value = value_type
                    .array_value(db)
                    .unwrap_or_else(|| TypeId::mixed(db));
                joined_key = Some(joined_key.map_or(spread_key, |k| join(db, k, spread_key)));
                joined_value =
                    Some(joined_value.map_or(spread_value, |v| join(db, v, spread_value)));
                continue;
            }
            let shape_key = match key_type {
                None => {
                    let index = next_index;
                    next_index += 1;
                    Some(crate::representation::ShapeKey::Integer(index))
                }
                Some(of) => {
                    is_list = false;
                    of.int_literal_value(db)
                        .map(crate::representation::ShapeKey::Integer)
                        .or_else(|| {
                            of.string_literal_value(db)
                                .map(crate::representation::ShapeKey::String)
                        })
                }
            };
            match shape_key {
                Some(shape_key) if shape_holds => fields.push(crate::representation::ShapeField {
                    key: shape_key,
                    optional: false,
                    value: value_type,
                }),
                _ => shape_holds = false,
            }
            let this_key = key_type.unwrap_or_else(|| TypeId::int(db));
            joined_key = Some(joined_key.map_or(this_key, |k| join(db, k, this_key)));
            joined_value = Some(joined_value.map_or(value_type, |v| join(db, v, value_type)));
        }
        if shape_holds && !fields.is_empty() {
            return TypeId::shape(db, fields);
        }
        match (joined_key, joined_value) {
            (Some(key), Some(value)) => {
                if is_list {
                    TypeId::non_empty_list(db, value)
                } else {
                    TypeId::non_empty_array(db, key, value)
                }
            }
            // The empty literal.
            _ => TypeId::shape(db, vec![]),
        }
    }

    /// One assignment: propagate to the target's subject, updating
    /// array bases on index writes and destructuring element-wise.
    /// Answers the expression's own type (the assigned value; the
    /// computed value for compound forms).
    pub(super) fn assignment(
        &mut self,
        operator: SyntaxKind,
        by_reference: bool,
        target: ExpressionId,
        _value: ExpressionId,
        value_type: TypeId<'db>,
        environment: &mut Environment<'db>,
    ) -> TypeId<'db> {
        let db = self.db();
        if operator == SyntaxKind::QuestionQuestionEquals {
            // `$x ??= v` reduces to `$x = $x ?? v`: the same
            // gated-widen-then-union combination as the `??` arm (see
            // its comment) so a same-family literal absorbs
            // (`?int $x; $x ??= 0;` answers `int`) while a genuinely
            // different alternative — or a pre-existing union — survives
            // instead of collapsing. The value operand was already
            // walked unconditionally by the `Assignment` arm above — its
            // environment effects apply on both paths, a recorded
            // conservative approximation.
            let current = self.recorded(target);
            let assigned = TypeId::union(
                db,
                [
                    widen_if_literal(db, current.without_null(db)),
                    widen_if_literal(db, value_type),
                ],
            );
            self.assign_target(target, assigned, environment);
            return assigned;
        }
        if by_reference {
            // `$b = &$a`: aliased locals are unknowable without alias
            // analysis — both sides degrade to mixed.
            if let Some(subject) = subject_of(self.context.ir, target) {
                environment.kill_call_results_for_subject(&subject);
                environment.bind(subject, TypeId::mixed(db));
            }
            if let Some(subject) = subject_of(self.context.ir, _value) {
                environment.kill_call_results_for_subject(&subject);
                environment.bind(subject, TypeId::mixed(db));
            }
            return TypeId::mixed(db);
        }
        let assigned = match compound_base(operator) {
            Some(base) => {
                let current = self.recorded(target);
                operators::binary_type(db, base, current, value_type)
            }
            None => value_type,
        };
        self.assign_target(target, assigned, environment);
        assigned
    }

    pub(super) fn assign_target(
        &mut self,
        target: ExpressionId,
        value_type: TypeId<'db>,
        environment: &mut Environment<'db>,
    ) {
        let db = self.db();
        match self.context.ir.expression(target).cloned() {
            // Destructuring: `[$a, $b] = ...`, `['k' => $v] = ...`.
            Some(BodyExpression::Array { entries }) => {
                let mut next_index: i64 = 0;
                for entry in &entries {
                    let ArrayEntry::Element { key, value, .. } = entry else {
                        next_index += 1;
                        continue;
                    };
                    let key_type = match key {
                        Some(key) => Some(self.recorded(*key)),
                        None => {
                            let index = next_index;
                            next_index += 1;
                            Some(TypeId::int_literal(db, index))
                        }
                    };
                    let element = operators::index_type(db, value_type, key_type);
                    self.assign_target(*value, element, environment);
                }
            }
            // An index write rebinds the base array.
            Some(BodyExpression::Index { subject, index }) => {
                if let Some(base) = subject_of(self.context.ir, subject) {
                    let current = environment.binding(&base);
                    let key_type = index.map(|index| self.recorded(index));
                    let updated = updated_array(db, current, key_type, value_type);
                    environment.kill_call_results_for_subject(&base);
                    environment.bind(base, updated);
                }
            }
            _ => {
                if let Some(subject) = subject_of(self.context.ir, target) {
                    environment.kill_call_results_for_subject(&subject);
                    environment.bind(subject, value_type);
                }
            }
        }
    }
}

/// Widen `of` to its general type only when it is a single-value
/// narrowing literal; anything else (a plain scalar, a class, or a
/// pre-existing union) passes through untouched. The `??` and `??=`
/// result types union their two operands after this gate, so a
/// same-family literal still absorbs (`?string ?? 'd'` → `string`)
/// while a multi-literal union survives (`(1|2|null) ?? 3` → `1|2|int`)
/// rather than collapsing the way widening the whole operand would.
pub(super) fn widen_if_literal<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> TypeId<'db> {
    if crate::narrowing::is_narrowing_literal(db, of) {
        widened_literals(db, of)
    } else {
        of
    }
}

/// `$a op= $b` reduces to `op`; `None` for plain `=` (and for `??=`,
/// which the walker handles separately).
fn compound_base(operator: SyntaxKind) -> Option<SyntaxKind> {
    Some(match operator {
        SyntaxKind::PlusEquals => SyntaxKind::Plus,
        SyntaxKind::MinusEquals => SyntaxKind::Minus,
        SyntaxKind::StarEquals => SyntaxKind::Star,
        SyntaxKind::SlashEquals => SyntaxKind::Slash,
        SyntaxKind::DotEquals => SyntaxKind::Dot,
        SyntaxKind::PercentEquals => SyntaxKind::Percent,
        SyntaxKind::StarStarEquals => SyntaxKind::StarStar,
        SyntaxKind::AmpersandEquals => SyntaxKind::Ampersand,
        SyntaxKind::PipeEquals => SyntaxKind::Pipe,
        SyntaxKind::CaretEquals => SyntaxKind::Caret,
        SyntaxKind::LessLessEquals => SyntaxKind::LessLess,
        SyntaxKind::GreaterGreaterEquals => SyntaxKind::GreaterGreater,
        _ => return None,
    })
}

/// The new type of an array base after `$a[k] = v`: a shape upserts
/// the field when the key is a known literal, an array joins key and
/// value, anything else becomes an array from this write.
fn updated_array<'db>(
    db: &'db dyn salsa::Database,
    current: Option<TypeId<'db>>,
    key_type: Option<TypeId<'db>>,
    value_type: TypeId<'db>,
) -> TypeId<'db> {
    use crate::representation::{ShapeField, ShapeKey};
    let literal_key = key_type.and_then(|key| {
        key.int_literal_value(db)
            .map(ShapeKey::Integer)
            .or_else(|| key.string_literal_value(db).map(ShapeKey::String))
    });
    if let Some(current) = current {
        if let Some(mut fields) = current.shape_fields(db) {
            match (&literal_key, key_type) {
                (Some(wanted), _) => {
                    fields.retain(|field| field.key != *wanted);
                    fields.push(ShapeField {
                        key: wanted.clone(),
                        optional: false,
                        value: value_type,
                    });
                    return TypeId::shape(db, fields);
                }
                (None, None) => {
                    // `$a[] = v`: the next free integer key.
                    let next = fields
                        .iter()
                        .filter_map(|field| match &field.key {
                            ShapeKey::Integer(index) => Some(*index + 1),
                            ShapeKey::String(_) => None,
                        })
                        .max()
                        .unwrap_or(0);
                    fields.push(ShapeField {
                        key: ShapeKey::Integer(next),
                        optional: false,
                        value: value_type,
                    });
                    return TypeId::shape(db, fields);
                }
                (None, Some(_)) => {
                    // A dynamic key on a shape: degrade to the array
                    // of the joined parts.
                    let (key, value) = shape_join(db, &fields);
                    let key_join = key_type.map_or(key, |of| join(db, key, of));
                    return TypeId::array(db, key_join, join(db, value, value_type));
                }
            }
        }
        if let (Some(key), Some(value)) = (current.array_key(db), current.array_value(db)) {
            let pushed_key = key_type.unwrap_or_else(|| TypeId::int(db));
            return TypeId::non_empty_array(
                db,
                join(db, key, pushed_key),
                join(db, value, value_type),
            );
        }
    }
    // Anything else (absent, mixed, scalar): the write makes it an
    // array from here on.
    match (literal_key, key_type) {
        (Some(key), _) => TypeId::shape(
            db,
            vec![ShapeField {
                key,
                optional: false,
                value: value_type,
            }],
        ),
        (None, None) => TypeId::non_empty_list(db, value_type),
        (None, Some(key)) => TypeId::non_empty_array(db, key, value_type),
    }
}

fn shape_join<'db>(
    db: &'db dyn salsa::Database,
    fields: &[crate::representation::ShapeField<'db>],
) -> (TypeId<'db>, TypeId<'db>) {
    use crate::representation::ShapeKey;
    let mut key: Option<TypeId<'db>> = None;
    let mut value: Option<TypeId<'db>> = None;
    for field in fields {
        let field_key = match &field.key {
            ShapeKey::Integer(index) => TypeId::int_literal(db, *index),
            ShapeKey::String(text) => TypeId::string_literal(db, text),
        };
        key = Some(key.map_or(field_key, |k| join(db, k, field_key)));
        value = Some(value.map_or(field.value, |v| join(db, v, field.value)));
    }
    (
        key.unwrap_or_else(|| TypeId::union(db, [TypeId::int(db), TypeId::string(db)])),
        value.unwrap_or_else(|| TypeId::mixed(db)),
    )
}
