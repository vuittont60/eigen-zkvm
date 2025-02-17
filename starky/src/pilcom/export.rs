//! porting it from powdr
use number::FieldElement;
use std::cmp;
use std::collections::HashMap;

use crate::types::{
    ConnectionIdentity, Expression as StarkyExpr, PermutationIdentity, PlookupIdentity,
    PolIdentity, Public, Reference, PIL,
};
use ast::analyzed::{
    Analyzed, BinaryOperator, Expression, FunctionValueDefinition, IdentityKind, PolyID,
    PolynomialReference, PolynomialType, Reference::*, StatementIdentifier, SymbolKind,
    UnaryOperator,
};

use super::expression_counter::compute_intermediate_expression_ids;

const DEFAULT_EXPR: StarkyExpr = StarkyExpr {
    op: String::new(),
    deg: 0,
    id: None,
    next: None,
    value: None,
    values: None,
    keep: None,
    keep2ns: None,
    idQ: None,
    const_: None,
};
struct Exporter<'a, T> {
    analyzed: &'a Analyzed<T>,
    expressions: Vec<StarkyExpr>,
    /// Translates from polynomial IDs to expression IDs for intermediate
    /// polynomials.
    intermediate_poly_expression_ids: HashMap<u64, u64>,
    number_q: u64,
}

pub fn export<T: FieldElement>(analyzed: &Analyzed<T>) -> PIL {
    let mut exporter = Exporter::new(analyzed);
    let mut publics = Vec::new();
    let mut pol_identities = Vec::new();
    let mut plookup_identities = Vec::new();
    let mut permutation_identities = Vec::new();
    let mut connection_identities = Vec::new();
    for item in &analyzed.source_order {
        match item {
            StatementIdentifier::Definition(name) => {
                if let (poly, Some(value)) = &analyzed.definitions[name] {
                    if poly.kind == SymbolKind::Poly(PolynomialType::Intermediate) {
                        if let FunctionValueDefinition::Expression(value) = value {
                            let expression_id = exporter.extract_expression(value, 1);
                            assert_eq!(
                                expression_id,
                                exporter.intermediate_poly_expression_ids[&poly.id] as usize
                            );
                        } else {
                            panic!("Expected single value");
                        }
                    }
                }
            }
            StatementIdentifier::PublicDeclaration(name) => {
                let pub_def = &analyzed.public_declarations[name];
                let (_, expr) = exporter.polynomial_reference_to_json(&pub_def.polynomial);
                let id = publics.len();
                publics.push(Public {
                    polType: polynomial_reference_type_to_type(&expr.op).to_string(),
                    polId: expr.id.unwrap(),
                    idx: pub_def.index as usize,
                    id,
                    name: name.clone(),
                });
            }
            StatementIdentifier::Identity(id) => {
                let identity = &analyzed.identities[*id];
                let file_name = identity.source.file.clone();
                let line = identity.source.line;
                let selector_degree = if identity.kind == IdentityKind::Polynomial {
                    2
                } else {
                    1
                };
                let left = exporter.extract_expression_vec(&identity.left.expressions, 1);
                let sel_left =
                    exporter.extract_expression_opt(&identity.left.selector, selector_degree);
                let right = exporter.extract_expression_vec(&identity.right.expressions, 1);
                let sel_right = exporter.extract_expression_opt(&identity.right.selector, 1);
                match identity.kind {
                    IdentityKind::Polynomial => pol_identities.push(PolIdentity {
                        e: sel_left.unwrap(),
                        fileName: file_name,
                        line,
                    }),
                    IdentityKind::Plookup => {
                        plookup_identities.push(PlookupIdentity {
                            selF: sel_left,
                            f: Some(left),
                            selT: sel_right,
                            t: Some(right),
                            fileName: file_name,
                            line,
                        });
                    }
                    IdentityKind::Permutation => {
                        permutation_identities.push(PermutationIdentity {
                            selF: sel_left,
                            f: Some(left),
                            selT: sel_right,
                            t: Some(right),
                            fileName: file_name,
                            line,
                        });
                    }
                    IdentityKind::Connect => {
                        connection_identities.push(ConnectionIdentity {
                            pols: Some(left),
                            connections: Some(right),
                            fileName: file_name,
                            line,
                        });
                    }
                }
            }
        }
    }
    PIL {
        nCommitments: analyzed.commitment_count(),
        nQ: exporter.number_q as usize,
        nIm: analyzed.intermediate_count(),
        nConstants: analyzed.constant_count(),
        publics,
        references: exporter.references(),
        expressions: exporter.expressions,
        polIdentities: pol_identities,
        plookupIdentities: plookup_identities,
        permutationIdentities: Some(permutation_identities),
        connectionIdentities: Some(connection_identities),
        cm_dims: Vec::new(),
        q2exp: Vec::new(),
    }
}

fn symbol_kind_to_json_string(k: SymbolKind) -> &'static str {
    match k {
        SymbolKind::Poly(poly_type) => polynomial_type_to_json_string(poly_type),
        SymbolKind::Other() => panic!("Cannot translate \"other\" symbol to json."),
    }
}

fn polynomial_type_to_json_string(t: PolynomialType) -> &'static str {
    polynomial_reference_type_to_type(polynomial_reference_type_to_json_string(t))
}

fn polynomial_reference_type_to_json_string(t: PolynomialType) -> &'static str {
    match t {
        PolynomialType::Committed => "cm",
        PolynomialType::Constant => "const",
        PolynomialType::Intermediate => "exp",
    }
}

fn polynomial_reference_type_to_type(t: &str) -> &'static str {
    match t {
        "cm" => "cmP",
        "const" => "constP",
        "exp" => "imP",
        _ => panic!("Invalid polynomial reference type {t}"),
    }
}

impl<'a, T: FieldElement> Exporter<'a, T> {
    fn new(analyzed: &'a Analyzed<T>) -> Self {
        Self {
            analyzed,
            expressions: vec![],
            intermediate_poly_expression_ids: compute_intermediate_expression_ids(analyzed),
            number_q: 0,
        }
    }

    fn references(&self) -> HashMap<String, Reference> {
        self.analyzed
            .definitions
            .iter()
            .map(|(name, (symbol, _value))| {
                let id = if symbol.kind == SymbolKind::Poly(PolynomialType::Intermediate) {
                    self.intermediate_poly_expression_ids[&symbol.id]
                } else {
                    symbol.id
                };
                let out = Reference {
                    polType: None,
                    type_: symbol_kind_to_json_string(symbol.kind).to_string(),
                    id: id as usize,
                    polDeg: symbol.degree as usize,
                    isArray: symbol.is_array(),
                    elementType: None,
                    len: symbol.length.map(|l| l as usize),
                };
                (name.clone(), out)
            })
            .collect::<HashMap<String, Reference>>()
    }

    /// Processes the given expression
    /// @returns the expression ID
    fn extract_expression(&mut self, expr: &Expression<T>, max_degree: u32) -> usize {
        let id = self.expressions.len();
        let (degree, mut expr) = self.expression_to_json(expr);
        if degree > max_degree {
            expr.idQ = Some(self.number_q as usize);
            expr.deg = 1;
            self.number_q += 1;
        }
        self.expressions.push(expr);
        id
    }

    fn extract_expression_opt(
        &mut self,
        expr: &Option<Expression<T>>,
        max_degree: u32,
    ) -> Option<usize> {
        expr.as_ref()
            .map(|e| self.extract_expression(e, max_degree))
    }

    fn extract_expression_vec(&mut self, expr: &[Expression<T>], max_degree: u32) -> Vec<usize> {
        expr.iter()
            .map(|e| self.extract_expression(e, max_degree))
            .collect()
    }

    /// returns the degree and the JSON value (intermediate polynomial IDs)
    fn expression_to_json(&self, expr: &Expression<T>) -> (u32, StarkyExpr) {
        match expr {
            Expression::Constant(name) => (
                0,
                StarkyExpr {
                    op: "number".to_string(),
                    deg: 0,
                    value: Some(format!("{}", self.analyzed.constants[name])),
                    ..DEFAULT_EXPR
                },
            ),
            Expression::Reference(Poly(reference)) => self.polynomial_reference_to_json(reference),
            Expression::Reference(LocalVar(_, _)) => {
                panic!("No local variable references allowed here.")
            }
            Expression::PublicReference(name) => (
                0,
                StarkyExpr {
                    op: "public".to_string(),
                    deg: 0,
                    id: Some(self.analyzed.public_declarations[name].id as usize),
                    ..DEFAULT_EXPR
                },
            ),
            Expression::Number(value) => (
                0,
                StarkyExpr {
                    op: "number".to_string(),
                    deg: 0,
                    value: Some(format!("{value}")),
                    ..DEFAULT_EXPR
                },
            ),
            Expression::BinaryOperation(left, op, right) => {
                let (deg_left, left) = self.expression_to_json(left);
                let (deg_right, right) = self.expression_to_json(right);
                let (op, degree) = match op {
                    BinaryOperator::Add => ("add", cmp::max(deg_left, deg_right)),
                    BinaryOperator::Sub => ("sub", cmp::max(deg_left, deg_right)),
                    BinaryOperator::Mul => ("mul", deg_left + deg_right),
                    BinaryOperator::Div => panic!("Div is not really allowed"),
                    BinaryOperator::Pow => {
                        assert_eq!(
                            deg_left + deg_right,
                            0,
                            "Exponentiation can only be used on constants."
                        );
                        ("pow", deg_left + deg_right)
                    }
                    BinaryOperator::Mod
                    | BinaryOperator::BinaryAnd
                    | BinaryOperator::BinaryOr
                    | BinaryOperator::BinaryXor
                    | BinaryOperator::ShiftLeft
                    | BinaryOperator::ShiftRight
                    | BinaryOperator::LogicalOr
                    | BinaryOperator::LogicalAnd
                    | BinaryOperator::Less
                    | BinaryOperator::LessEqual
                    | BinaryOperator::Equal
                    | BinaryOperator::NotEqual
                    | BinaryOperator::GreaterEqual
                    | BinaryOperator::Greater => {
                        panic!("Operator {op:?} not supported on polynomials.")
                    }
                };
                (
                    degree,
                    StarkyExpr {
                        op: op.to_string(),
                        deg: degree as usize,
                        values: Some(vec![left, right]),
                        ..DEFAULT_EXPR
                    },
                )
            }
            Expression::UnaryOperation(op, value) => {
                let (deg, value) = self.expression_to_json(value);
                match op {
                    UnaryOperator::Plus => (deg, value),
                    UnaryOperator::Minus => (
                        deg,
                        StarkyExpr {
                            op: "neg".to_string(),
                            deg: deg as usize,
                            values: Some(vec![value]),
                            ..DEFAULT_EXPR
                        },
                    ),
                    UnaryOperator::LogicalNot => panic!("Operator {op} not allowed here."),
                }
            }
            Expression::FunctionCall(_) => panic!("No function calls allowed here."),
            Expression::String(_) => panic!("Strings not allowed here."),
            Expression::Tuple(_) => panic!("Tuples not allowed here"),
            Expression::ArrayLiteral(_) => panic!("Array literals not allowed here"),
            Expression::MatchExpression(_, _) => {
                panic!("No match expressions allowed here.")
            }
            Expression::LambdaExpression(_) => {
                panic!("No lambda expressions allowed here.")
            }
            Expression::FreeInput(_) => {
                panic!("No free input expressions allowed here.")
            }
        }
    }

    fn polynomial_reference_to_json(
        &self,
        PolynomialReference {
            name: _,
            index,
            poly_id,
            next,
        }: &PolynomialReference,
    ) -> (u32, StarkyExpr) {
        let PolyID { id, ptype } = poly_id.unwrap();
        let id = if ptype == PolynomialType::Intermediate {
            assert!(index.is_none());
            self.intermediate_poly_expression_ids[&id]
        } else {
            id + index.unwrap_or_default()
        };
        let poly = StarkyExpr {
            id: Some(id as usize),
            op: polynomial_reference_type_to_json_string(ptype).to_string(),
            deg: 1,
            next: Some(*next),
            ..DEFAULT_EXPR
        };
        (1, poly)
    }
}
