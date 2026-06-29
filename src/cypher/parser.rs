use nom::{
    branch::alt,
    bytes::complete::{tag, tag_no_case, take_while},
    character::complete::{alpha1, char, multispace0, multispace1},
    combinator::{map, opt, recognize},
    multi::{many0, separated_list1},
    sequence::{delimited, pair, preceded, tuple},
    IResult,
};

#[derive(Debug, Clone, PartialEq)]
pub struct NodePattern {
    pub variable: Option<String>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RelDirection {
    Inbound,
    Outbound,
    Undirected,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RelPattern {
    pub variable: Option<String>,
    pub rel_type: Option<String>,
    pub direction: RelDirection,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PatternElement {
    Node(NodePattern),
    Rel(RelPattern, NodePattern), // rel-to-node
}

#[derive(Debug, Clone, PartialEq)]
pub enum WhereExpr {
    Eq(String, String),               // e.g. f.name = "main" or f.name = 'main'
    NotExistsRel(String, RelPattern), // e.g. NOT EXISTS { (f)<-[:calls]-() }
    And(Box<WhereExpr>, Box<WhereExpr>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderByClause {
    pub property: String, // e.g. "n.complexity"
    pub descending: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CypherQuery {
    pub match_pattern: Vec<PatternElement>,
    pub r#where: Option<WhereExpr>,
    pub r#return: Vec<String>, // e.g. ["f.name", "g.file_path"] or ["n"]
    pub order_by: Option<OrderByClause>,
    pub limit: Option<usize>,
}

// Identifier parser: alphanumeric or underscore starting with alpha/underscore
fn identifier(input: &str) -> IResult<&str, String> {
    map(
        recognize(pair(
            alt((alpha1, tag("_"))),
            take_while(|c: char| c.is_alphanumeric() || c == '_'),
        )),
        |s: &str| s.to_string(),
    )(input)
}

// Literal string: single or double quotes
fn string_literal(input: &str) -> IResult<&str, String> {
    alt((
        delimited(char('"'), take_while(|c: char| c != '"'), char('"')),
        delimited(char('\''), take_while(|c: char| c != '\''), char('\'')),
    ))(input)
    .map(|(i, s)| (i, s.to_string()))
}

// Node pattern: (n:Label) or (n) or (:Label)
fn node_pattern(input: &str) -> IResult<&str, NodePattern> {
    let inner = tuple((
        opt(identifier),
        opt(preceded(
            tuple((multispace0, char(':'), multispace0)),
            identifier,
        )),
    ));
    map(
        delimited(char('('), inner, char(')')),
        |(variable, label)| NodePattern { variable, label },
    )(input)
}

fn bracket_parser(input: &str) -> IResult<&str, (Option<String>, Option<String>)> {
    let bracket_inner = tuple((
        opt(identifier),
        opt(preceded(
            tuple((multispace0, char(':'), multispace0)),
            identifier,
        )),
    ));
    delimited(char('['), bracket_inner, char(']'))(input)
}

// Rel pattern: -[:TYPE]-> or <-[:TYPE]- or -[:TYPE]-
fn rel_pattern(input: &str) -> IResult<&str, RelPattern> {
    // Try parsing arrow forms
    alt((
        // <-[rel]-
        map(
            tuple((tag("<-"), opt(bracket_parser), tag("-"))),
            |(_, content, _)| {
                let (variable, rel_type) = content.unwrap_or((None, None));
                RelPattern {
                    variable,
                    rel_type,
                    direction: RelDirection::Inbound,
                }
            },
        ),
        // -[rel]->
        map(
            tuple((tag("-"), opt(bracket_parser), tag("->"))),
            |(_, content, _)| {
                let (variable, rel_type) = content.unwrap_or((None, None));
                RelPattern {
                    variable,
                    rel_type,
                    direction: RelDirection::Outbound,
                }
            },
        ),
        // -[rel]-
        map(
            tuple((tag("-"), opt(bracket_parser), tag("-"))),
            |(_, content, _)| {
                let (variable, rel_type) = content.unwrap_or((None, None));
                RelPattern {
                    variable,
                    rel_type,
                    direction: RelDirection::Undirected,
                }
            },
        ),
    ))(input)
}

// Match pattern path: (n)-[:calls]->(m) ...
pub fn match_pattern(input: &str) -> IResult<&str, Vec<PatternElement>> {
    let (mut input, start_node) = node_pattern(input)?;
    let mut elements = vec![PatternElement::Node(start_node)];

    loop {
        let (next_input, rel) = match opt(rel_pattern)(input) {
            Ok((i, Some(r))) => (i, r),
            _ => break,
        };
        let (next_input2, target_node) = node_pattern(next_input)?;
        elements.push(PatternElement::Rel(rel, target_node));
        input = next_input2;
    }

    Ok((input, elements))
}

// Property access: n.name or n.file_path
fn property_access(input: &str) -> IResult<&str, (String, String)> {
    map(
        tuple((identifier, char('.'), identifier)),
        |(var, _, prop)| (var, prop),
    )(input)
}

// Not exists block e.g. NOT EXISTS { (f)<-[:calls]-() }
fn not_exists_expr(input: &str) -> IResult<&str, WhereExpr> {
    let (input, _) = tag_no_case("NOT")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("EXISTS")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('{')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, start_var) = delimited(char('('), opt(identifier), char(')'))(input)?;
    let (input, rel) = rel_pattern(input)?;
    let (input, _) = delimited(char('('), opt(identifier), char(')'))(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('}')(input)?;

    Ok((
        input,
        WhereExpr::NotExistsRel(start_var.unwrap_or_default(), rel),
    ))
}

// Simple equality comparison e.g. n.name = "foo"
fn eq_expr(input: &str) -> IResult<&str, WhereExpr> {
    let (input, (var, prop)) = property_access(input)?;
    let (input, _) = tuple((multispace0, char('='), multispace0))(input)?;
    let (input, val) = string_literal(input)?;
    Ok((input, WhereExpr::Eq(format!("{}.{}", var, prop), val)))
}

fn where_expr(input: &str) -> IResult<&str, WhereExpr> {
    alt((not_exists_expr, eq_expr))(input)
}

// Full Where expression parser, optionally with AND
fn full_where_clause(input: &str) -> IResult<&str, WhereExpr> {
    let (input, first) = where_expr(input)?;

    // Check for AND
    let and_parser = preceded(
        tuple((multispace1, tag_no_case("AND"), multispace1)),
        where_expr,
    );
    let (input, chain) = many0(and_parser)(input)?;

    let mut result = first;
    for next in chain {
        result = WhereExpr::And(Box::new(result), Box::new(next));
    }

    Ok((input, result))
}

// Parse RETURN clause: RETURN n.name, m
fn return_clause(input: &str) -> IResult<&str, Vec<String>> {
    let element_parser = map(
        alt((
            map(property_access, |(v, p)| format!("{}.{}", v, p)),
            identifier,
        )),
        |s| s,
    );
    separated_list1(tuple((multispace0, char(','), multispace0)), element_parser)(input)
}

// Parse LIMIT clause: LIMIT 50
fn limit_clause(input: &str) -> IResult<&str, usize> {
    let (input, digits) = take_while(|c: char| c.is_ascii_digit())(input)?;
    let val = digits.parse::<usize>().unwrap_or(0);
    Ok((input, val))
}

// Parse ORDER BY clause: ORDER BY n.complexity DESC or ORDER BY n.name ASC
fn order_by_clause(input: &str) -> IResult<&str, OrderByClause> {
    let (input, prop) = property_access(input)?;
    let property = format!("{}.{}", prop.0, prop.1);
    let (input, dir) = opt(preceded(
        multispace1,
        alt((tag_no_case("DESC"), tag_no_case("ASC"))),
    ))(input)?;
    let descending = dir.map(|d| d.to_uppercase() == "DESC").unwrap_or(false);
    Ok((
        input,
        OrderByClause {
            property,
            descending,
        },
    ))
}

// Complete Cypher query parser
pub fn parse_cypher(input: &str) -> Result<CypherQuery, String> {
    let input = input.trim();
    let to_err = |e: nom::Err<nom::error::Error<&str>>| e.to_string();

    // MATCH clause
    let (input, _) = tag_no_case("MATCH")(input).map_err(to_err)?;
    let (input, _) = multispace1(input).map_err(to_err)?;
    let (input, pattern) = match_pattern(input).map_err(to_err)?;

    // Optional WHERE clause
    let (input, w_expr) = match opt(preceded(
        tuple((multispace1, tag_no_case("WHERE"), multispace1)),
        full_where_clause,
    ))(input)
    {
        Ok((i, w)) => (i, w),
        Err(e) => return Err(to_err(e)),
    };

    // RETURN clause
    let (input, _) = preceded(multispace1, tag_no_case("RETURN"))(input).map_err(to_err)?;
    let (input, _) = multispace1(input).map_err(to_err)?;
    let (input, ret) = return_clause(input).map_err(to_err)?;

    // Optional ORDER BY clause
    let (input, order) = match opt(preceded(
        tuple((
            multispace1,
            tag_no_case("ORDER"),
            multispace1,
            tag_no_case("BY"),
            multispace1,
        )),
        order_by_clause,
    ))(input)
    {
        Ok((i, o)) => (i, o),
        _ => (input, None),
    };

    // Optional LIMIT clause
    let (_, limit) = match opt(preceded(
        tuple((multispace1, tag_no_case("LIMIT"), multispace1)),
        limit_clause,
    ))(input)
    {
        Ok((_, l)) => (input, l),
        _ => (input, None),
    };

    Ok(CypherQuery {
        match_pattern: pattern,
        r#where: w_expr,
        r#return: ret,
        order_by: order,
        limit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        let q = "MATCH (n:Function) WHERE n.name = 'main' RETURN n LIMIT 10";
        let parsed = parse_cypher(q).unwrap();
        assert_eq!(
            parsed.match_pattern,
            vec![PatternElement::Node(NodePattern {
                variable: Some("n".to_string()),
                label: Some("Function".to_string())
            })]
        );
        assert_eq!(parsed.r#return, vec!["n".to_string()]);
        assert_eq!(parsed.limit, Some(10));
        assert_eq!(
            parsed.r#where,
            Some(WhereExpr::Eq("n.name".to_string(), "main".to_string()))
        );
    }

    #[test]
    fn test_parse_calls() {
        let q = "MATCH (f)-[:calls]->(g) RETURN f.name, g.name";
        let parsed = parse_cypher(q).unwrap();
        assert_eq!(parsed.match_pattern.len(), 2);
        assert_eq!(
            parsed.r#return,
            vec!["f.name".to_string(), "g.name".to_string()]
        );
    }

    #[test]
    fn test_parse_not_exists() {
        let q = "MATCH (f:Function) WHERE NOT EXISTS { (f)<-[:calls]-() } RETURN f.name";
        let parsed = parse_cypher(q).unwrap();
        assert_eq!(parsed.match_pattern.len(), 1);
        assert_eq!(
            parsed.r#where,
            Some(WhereExpr::NotExistsRel(
                "f".to_string(),
                RelPattern {
                    variable: None,
                    rel_type: Some("calls".to_string()),
                    direction: RelDirection::Inbound
                }
            ))
        );
    }
}
