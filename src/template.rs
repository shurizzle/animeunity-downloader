use core::fmt;
use std::{
    collections::HashMap,
    iter::FusedIterator,
    ops::{RangeFrom, RangeTo},
};

use nom::{
    branch::alt,
    bytes::complete::{tag, take_while, take_while1},
    character::complete::{char, one_of},
    combinator::{all_consuming, map},
    error::{ErrorKind, ParseError},
    multi::{fold_many1, fold_many_m_n, many0},
    sequence::{delimited, pair, preceded, terminated},
    AsChar, Compare, Err, ExtendInto, IResult, InputIter, InputLength, InputTake,
    InputTakeAtPosition, Offset, Slice,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Item {
    Variable(Box<str>),
    Text(Box<str>),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Template(Box<[Item]>);

pub struct VarIter<'a>(std::slice::Iter<'a, Item>);

pub trait Variables {
    type Item<'a>: fmt::Display
    where
        Self: 'a;

    #[allow(clippy::needless_lifetimes)]
    fn get<'a, S: AsRef<str>>(&'a self, name: S) -> Option<Self::Item<'a>>;
}

#[derive(Debug)]
pub struct BoundTemplate<'a, V: Variables>(&'a Template, &'a V);

impl<'a, V: Variables> fmt::Display for BoundTemplate<'a, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for item in self.0 .0.iter() {
            match item {
                Item::Variable(name) => {
                    if let Some(value) = self.1.get(name) {
                        write!(f, "{}", value)?;
                    }
                }
                Item::Text(txt) => write!(f, "{}", &**txt)?,
            }
        }
        Ok(())
    }
}

impl<T: fmt::Display> Variables for HashMap<Box<str>, T> {
    type Item<'a> = &'a T
    where
        Self: 'a;

    #[allow(clippy::needless_lifetimes)]
    fn get<'a, S: AsRef<str>>(&'a self, name: S) -> Option<Self::Item<'a>> {
        <HashMap<Box<str>, T>>::get(self, name.as_ref())
    }
}

impl<T: fmt::Display> Variables for HashMap<String, T> {
    type Item<'a> = &'a T
    where
        Self: 'a;

    #[allow(clippy::needless_lifetimes)]
    fn get<'a, S: AsRef<str>>(&'a self, name: S) -> Option<Self::Item<'a>> {
        <HashMap<String, T>>::get(self, name.as_ref())
    }
}

impl<'a, T: fmt::Display> Variables for HashMap<&'a str, T> {
    type Item<'b> = &'b T
    where
        Self: 'b;

    fn get<'b, S: AsRef<str>>(&'b self, name: S) -> Option<Self::Item<'b>> {
        <HashMap<&'a str, T>>::get(self, name.as_ref())
    }
}

impl<'a> fmt::Debug for VarIter<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Variables").field(&self.0.clone()).finish()
    }
}

impl<'a> Iterator for VarIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Item::Variable(name) = self.0.next()? {
                return Some(&**name);
            }
        }
    }
}

impl<'a> DoubleEndedIterator for VarIter<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        loop {
            if let Item::Variable(name) = self.0.next_back()? {
                return Some(&**name);
            }
        }
    }
}

impl<'a> FusedIterator for VarIter<'a> {}

impl Template {
    pub fn parse<S: AsRef<str>>(input: S) -> Option<Template> {
        parser::<_, (_, ErrorKind)>(input.as_ref())
            .ok()
            .map(|(_, templ)| templ)
    }

    pub fn variables(&self) -> VarIter {
        VarIter(self.0.iter())
    }

    pub fn bind<'a, T: Variables>(&'a self, vars: &'a T) -> BoundTemplate<'a, T> {
        BoundTemplate(self, vars)
    }

    pub fn render<T: Variables>(&self, vars: &T) -> Box<str> {
        self.bind(vars).to_string().into_boxed_str()
    }
}

fn hexdigit<Input, Error>(input: Input) -> IResult<Input, u32, Error>
where
    Input: InputIter + Slice<RangeFrom<usize>> + Clone,
    <Input as InputIter>::Item: AsChar + Copy,
    &'static str: nom::FindToken<<Input as nom::InputIter>::Item>,
    Error: ParseError<Input>,
{
    map(one_of("0123456789abcdefABCDEF"), |c: char| match c {
        '0'..='9' => (c as u32) - ('0' as u32),
        'a'..='f' => (c as u32) - ('a' as u32) + 10,
        'A'..='F' => (c as u32) - ('A' as u32) + 10,
        _ => unsafe { core::hint::unreachable_unchecked() },
    })(input)
}

fn unicode_escaped<Input, Error>(input: Input) -> IResult<Input, char, Error>
where
    Input: InputLength
        + InputIter
        + Slice<RangeFrom<usize>>
        + InputTakeAtPosition<Item = char>
        + Clone,
    <Input as InputIter>::Item: AsChar + Copy,
    &'static str: nom::FindToken<<Input as nom::InputIter>::Item>,
    Error: ParseError<Input>,
{
    let initial = input.clone();

    let (input, n) = fold_many_m_n(
        1,
        6,
        terminated(hexdigit, take_while(|c: char| c == '_')),
        || 0,
        |acc, n| acc * 10 + n,
    )(input)?;

    if let Some(c) = char::from_u32(n) {
        Ok((input, c))
    } else {
        Err(Err::Error(Error::from_error_kind(initial, ErrorKind::Char)))
    }
}

fn octdigit<Input, Error>(input: Input) -> IResult<Input, u32, Error>
where
    Input: InputIter + Slice<RangeFrom<usize>> + Clone,
    <Input as InputIter>::Item: AsChar + Copy,
    &'static str: nom::FindToken<<Input as nom::InputIter>::Item>,
    Error: ParseError<Input>,
{
    map(one_of("01234567"), |c: char| (c as u32) - ('0' as u32))(input)
}

fn text<Input, Error>(input: Input) -> IResult<Input, Item, Error>
where
    Input: InputLength
        + InputIter<Item = char>
        + InputTake
        + InputTakeAtPosition<Item = char>
        + Compare<&'static str>
        + Offset
        + Slice<RangeTo<usize>>
        + Slice<RangeFrom<usize>>
        + ExtendInto<Extender = String>
        + Clone,
    Error: ParseError<Input>,
{
    enum S<Input: ExtendInto<Extender = String>> {
        I(Input),
        C(char),
    }
    impl<I: ExtendInto<Extender = String>> ExtendInto for S<I> {
        type Item = char;
        type Extender = String;
        fn new_builder(&self) -> Self::Extender {
            String::new()
        }
        fn extend_into(&self, acc: &mut Self::Extender) {
            match self {
                S::I(i) => i.extend_into(acc),
                &S::C(c) => acc.push(c),
            }
        }
    }

    map(
        fold_many1(
            alt((
                map(
                    take_while1(|c: char| !matches!(c, '{' | '}' | '\\' | '"')),
                    S::I,
                ),
                map(tag("{{"), |_| S::C('{')),
                map(tag("}}"), |_| S::C('}')),
                preceded(
                    char('\\'),
                    alt((
                        map(char('\\'), |_| S::C('\\')),
                        map(char('n'), |_| S::C('\n')),
                        map(char('r'), |_| S::C('\r')),
                        map(char('t'), |_| S::C('\t')),
                        map(char('a'), |_| S::C('\x07')),
                        map(char('"'), |_| S::C('"')),
                        map(char('\''), |_| S::C('\'')),
                        map(char('0'), |_| S::C('\0')),
                        preceded(
                            char('x'),
                            map(pair(octdigit, hexdigit), |(a, b)| {
                                S::C(unsafe { char::from_u32_unchecked(a * 10 + b) })
                            }),
                        ),
                        preceded(
                            char('u'),
                            delimited(char('{'), map(unicode_escaped, S::C), char('}')),
                        ),
                    )),
                ),
            )),
            String::new,
            |mut acc, i| {
                i.extend_into(&mut acc);
                acc
            },
        ),
        |s| Item::Text(s.into_boxed_str()),
    )(input)
}

fn variable<Input, Error>(input: Input) -> IResult<Input, Item, Error>
where
    Input: InputLength
        + InputTake
        + InputTakeAtPosition<Item = char>
        + InputIter<Item = char>
        + Slice<RangeFrom<usize>>
        + Slice<RangeTo<usize>>
        + ExtendInto<Extender = String>
        + Offset
        + Clone,
    Error: ParseError<Input>,
{
    map(
        delimited(
            char('{'),
            take_while1(|c: char| !matches!(c, '{' | '}' | '\\' | '"')),
            char('}'),
        ),
        |t: Input| {
            let mut res = String::new();
            t.extend_into(&mut res);
            Item::Variable(res.into_boxed_str())
        },
    )(input)
}

fn parser<Input, Error>(input: Input) -> IResult<Input, Template, Error>
where
    Input: InputLength
        + InputIter<Item = char>
        + InputTake
        + InputTakeAtPosition<Item = char>
        + Compare<&'static str>
        + Slice<RangeTo<usize>>
        + Slice<RangeFrom<usize>>
        + ExtendInto<Extender = String>
        + Offset
        + fmt::Debug
        + Clone,
    Error: ParseError<Input> + fmt::Debug,
{
    map(all_consuming(many0(alt((text, variable)))), |xs| {
        Template(xs.into())
    })(input)
}
