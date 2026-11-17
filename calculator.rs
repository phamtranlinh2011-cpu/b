//! Scientific calculator engine and ClassWiz-inspired utility calculations.
//! This is an independent implementation; it does not contain Casio firmware or UI assets.

use std::f64::consts::{E, PI};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AngleUnit { Degree, Radian, Gradian }

impl AngleUnit {
    pub fn to_radians(self, value: f64) -> f64 {
        match self { Self::Degree => value.to_radians(), Self::Radian => value, Self::Gradian => value * PI / 200.0 }
    }
    pub fn from_radians(self, value: f64) -> f64 {
        match self { Self::Degree => value.to_degrees(), Self::Radian => value, Self::Gradian => value * 200.0 / PI }
    }
    pub fn label(self) -> &'static str { match self { Self::Degree => "DEG", Self::Radian => "RAD", Self::Gradian => "GRA" } }
}

#[derive(Clone, Copy, Debug)]
pub struct EvalContext { pub angle: AngleUnit, pub ans: f64, pub x: f64 }

impl Default for EvalContext {
    fn default() -> Self { Self { angle: AngleUnit::Degree, ans: 0.0, x: 0.0 } }
}

#[derive(Clone, Debug, PartialEq)]
enum Token { Number(f64), Ident(String), Plus, Minus, Mul, Div, Pow, LParen, RParen, Comma, Factorial, Percent, End }

fn tokenize(source: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch.is_whitespace() { i += 1; continue; }
        let simple = match ch {
            '+' => Some(Token::Plus), '-' | '−' => Some(Token::Minus), '*' | '×' => Some(Token::Mul), '/' | '÷' => Some(Token::Div), '^' => Some(Token::Pow), '(' => Some(Token::LParen), ')' => Some(Token::RParen), ',' | ';' => Some(Token::Comma), '!' => Some(Token::Factorial), '%' => Some(Token::Percent), _ => None,
        };
        if let Some(token) = simple { out.push(token); i += 1; continue; }
        if ch.is_ascii_digit() || ch == '.' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') { i += 1; }
            if i < chars.len() && matches!(chars[i], 'e' | 'E') {
                i += 1;
                if i < chars.len() && matches!(chars[i], '+' | '-') { i += 1; }
                let exponent_start = i;
                while i < chars.len() && chars[i].is_ascii_digit() { i += 1; }
                if exponent_start == i { return Err("Invalid scientific notation".into()); }
            }
            let text: String = chars[start..i].iter().collect();
            let value = text.parse::<f64>().map_err(|_| format!("Invalid number: {text}"))?;
            if !value.is_finite() { return Err("Number is outside the supported range".into()); }
            out.push(Token::Number(value));
            continue;
        }
        if ch.is_alphabetic() || ch == '_' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') { i += 1; }
            out.push(Token::Ident(chars[start..i].iter().collect::<String>().to_ascii_lowercase()));
            continue;
        }
        return Err(format!("Unsupported character: {ch}"));
    }
    out.push(Token::End);
    Ok(out)
}

struct Parser { tokens: Vec<Token>, pos: usize, context: EvalContext }

impl Parser {
    fn peek(&self) -> &Token { &self.tokens[self.pos] }
    fn take(&mut self) -> Token { let token = self.tokens[self.pos].clone(); self.pos += 1; token }
    fn parse(mut self) -> Result<f64, String> {
        let value = self.expression(0)?;
        if self.peek() != &Token::End { return Err("Unexpected input after expression".into()); }
        finite(value)
    }
    fn expression(&mut self, min_bp: u8) -> Result<f64, String> {
        let mut lhs = match self.take() {
            Token::Number(n) => n,
            Token::Plus => self.expression(7)?,
            Token::Minus => -self.expression(7)?,
            Token::LParen => { let value = self.expression(0)?; if self.take() != Token::RParen { return Err("Missing closing parenthesis".into()); } value },
            Token::Ident(name) => self.identifier(name)?,
            other => return Err(format!("Expected a number or function, got {other:?}")),
        };
        loop {
            match self.peek() {
                Token::Factorial => {
                    if 9 < min_bp { break; }
                    self.take(); lhs = factorial(lhs)?; continue;
                }
                Token::Percent => {
                    if 9 < min_bp { break; }
                    self.take(); lhs /= 100.0; continue;
                }
                _ => {}
            }
            let (left_bp, right_bp, op) = match self.peek() {
                Token::Plus => (1, 2, 0), Token::Minus => (1, 2, 1),
                Token::Mul => (3, 4, 2), Token::Div => (3, 4, 3),
                Token::Pow => (8, 7, 4),
                // ClassWiz-style implicit multiplication: 2π, 3(4+1), 2sin(30).
                Token::Number(_) | Token::Ident(_) | Token::LParen => (3, 4, 5),
                _ => break,
            };
            if left_bp < min_bp { break; }
            if op != 5 { self.take(); }
            let rhs = self.expression(right_bp)?;
            lhs = match op {
                0 => lhs + rhs, 1 => lhs - rhs, 2 | 5 => lhs * rhs,
                3 if rhs != 0.0 => lhs / rhs, 3 => return Err("Cannot divide by zero".into()),
                4 => lhs.powf(rhs), _ => unreachable!(),
            };
            finite(lhs)?;
        }
        Ok(lhs)
    }
    fn identifier(&mut self, name: String) -> Result<f64, String> {
        if self.peek() != &Token::LParen {
            return match name.as_str() {
                "pi" | "π" => Ok(PI), "e" => Ok(E), "ans" => Ok(self.context.ans), "x" => Ok(self.context.x),
                "phi" => Ok((1.0 + 5.0_f64.sqrt()) / 2.0),
                "c" => Ok(299_792_458.0), "g0" => Ok(9.80665),
                "h_planck" => Ok(6.626_070_15e-34), "g_newton" => Ok(6.674_30e-11),
                "na" => Ok(6.022_140_76e23), "k_b" => Ok(1.380_649e-23),
                "m_e" => Ok(9.109_383_7139e-31), "m_p" => Ok(1.672_621_92595e-27),
                "q_e" => Ok(1.602_176_634e-19), "r_gas" => Ok(8.314_462_618),
                _ => Err(format!("Unknown variable or function: {name}")),
            };
        }
        self.take();
        let mut args = Vec::new();
        if self.peek() != &Token::RParen {
            loop {
                args.push(self.expression(0)?);
                if self.peek() == &Token::Comma { self.take(); continue; }
                break;
            }
        }
        if self.take() != Token::RParen { return Err(format!("Missing ')' after {name}")); }
        let one = || if args.len() == 1 { Ok(args[0]) } else { Err(format!("{name} expects one argument")) };
        let two = || if args.len() == 2 { Ok((args[0], args[1])) } else { Err(format!("{name} expects two arguments")) };
        let three = || if args.len() == 3 { Ok((args[0], args[1], args[2])) } else { Err(format!("{name} expects three arguments")) };
        let four = || if args.len() == 4 { Ok((args[0], args[1], args[2], args[3])) } else { Err(format!("{name} expects four arguments")) };
        let angle = self.context.angle;
        let value = match name.as_str() {
            "sin" => angle.to_radians(one()?).sin(), "cos" => angle.to_radians(one()?).cos(), "tan" => angle.to_radians(one()?).tan(),
            "sec" => { let v=angle.to_radians(one()?).cos(); if v.abs()<1e-15 { return Err("sec is undefined at this angle".into()); } 1.0/v },
            "csc" => { let v=angle.to_radians(one()?).sin(); if v.abs()<1e-15 { return Err("csc is undefined at this angle".into()); } 1.0/v },
            "cot" => { let v=angle.to_radians(one()?).tan(); if v.abs()<1e-15 { return Err("cot is undefined at this angle".into()); } 1.0/v },
            "asin" | "arcsin" => angle.from_radians(one()?.asin()), "acos" | "arccos" => angle.from_radians(one()?.acos()), "atan" | "arctan" => angle.from_radians(one()?.atan()),
            "sinh" => one()?.sinh(), "cosh" => one()?.cosh(), "tanh" => one()?.tanh(),
            "asinh" => one()?.asinh(), "acosh" => one()?.acosh(), "atanh" => one()?.atanh(),
            "sqrt" => one()?.sqrt(), "cbrt" => one()?.cbrt(), "abs" => one()?.abs(), "ln" => one()?.ln(), "log" | "log10" => one()?.log10(), "log2" => one()?.log2(),
            "exp" => one()?.exp(), "exp10" => 10.0_f64.powf(one()?), "floor" => one()?.floor(), "ceil" => one()?.ceil(), "round" => one()?.round(), "trunc" => one()?.trunc(), "sign" => one()?.signum(),
            "recip" | "inv" => { let v = one()?; if v == 0.0 { return Err("Cannot divide by zero".into()); } 1.0 / v },
            "sqr" => one()?.powi(2), "cube" => one()?.powi(3),
            "min" => { let (a,b)=two()?; a.min(b) }, "max" => { let (a,b)=two()?; a.max(b) }, "pow" => { let (a,b)=two()?; a.powf(b) },
            "logbase" => { let (a,b)=two()?; a.log(b) }, "mod" => { let (a,b)=two()?; a % b },
            "ncr" => { let (n,r)=two()?; permutation(n,r)? / factorial(r)? },
            "npr" => { let (n,r)=two()?; permutation(n,r)? },
            "atan2" => { let (y,x)=two()?; angle.from_radians(y.atan2(x)) },
            "normalpdf" => { let (x,mu,sigma)=three()?; if sigma<=0.0 { return Err("Normal distribution sigma must be positive".into()); } let z=(x-mu)/sigma; (-0.5*z*z).exp()/(sigma*(2.0*PI).sqrt()) },
            "normalcdf" => { let (lo,hi,mu,sigma)=four()?; if sigma<=0.0 || lo>hi { return Err("Normal CDF requires sigma > 0 and lower ≤ upper".into()); } 0.5*(erf_approx((hi-mu)/(sigma*2.0_f64.sqrt()))-erf_approx((lo-mu)/(sigma*2.0_f64.sqrt()))) },
            "binompdf" => { let (n,p,k)=three()?; binomial_probability(n,p,k)? },
            "binomcdf" => { let (n,p,k)=three()?; binomial_cumulative_probability(n,p,k)? },
            "poissonpdf" => { let (lambda,k)=two()?; poisson_probability(lambda,k)? },
            "poissoncdf" => { let (lambda,k)=two()?; poisson_cumulative_probability(lambda,k)? },
            "gcd" => { let (a,b)=two()?; gcd_integer(a,b)? as f64 },
            "lcm" => { let (a,b)=two()?; let g=gcd_integer(a,b)?; if g == 0 { 0.0 } else { ((a.abs() as i64 / g).checked_mul(b.abs() as i64).ok_or_else(|| "lcm result is too large".to_owned())?) as f64 } },
            "deg" => one()?.to_degrees(), "rad" => one()?.to_radians(),
            _ => return Err(format!("Unsupported function: {name}")),
        };
        finite(value)
    }
}

fn validate_count(value: f64, max: u32) -> Result<u32, String> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > max as f64 {
        Err(format!("Count must be an integer from 0 to {max}"))
    } else {
        Ok(value as u32)
    }
}

fn validate_binomial(n: f64, p: f64, k: f64) -> Result<(u32, f64, u32), String> {
    let n = validate_count(n, 10_000)?;
    let k = validate_count(k, n)?;
    if !p.is_finite() || !(0.0..=1.0).contains(&p) {
        return Err("Binomial probability p must be between 0 and 1".into());
    }
    Ok((n, p, k))
}

/// Evaluate a Binomial PMF in log space. Computing nCr directly overflows
/// long before the final probability does for large but valid parameter sets.
fn log_binomial_pmf(n: u32, p: f64, k: u32) -> f64 {
    if p == 0.0 { return if k == 0 { 0.0 } else { f64::NEG_INFINITY }; }
    if p == 1.0 { return if k == n { 0.0 } else { f64::NEG_INFINITY }; }

    let r = k.min(n - k);
    let mut log_combination = 0.0;
    let mut compensation = 0.0;
    for i in 1..=r {
        let term = ((n - r + i) as f64).ln() - (i as f64).ln();
        let adjusted = term - compensation;
        let next = log_combination + adjusted;
        compensation = (next - log_combination) - adjusted;
        log_combination = next;
    }
    log_combination + (k as f64) * p.ln() + ((n - k) as f64) * (-p).ln_1p()
}

fn binomial_probability(n: f64, p: f64, k: f64) -> Result<f64, String> {
    let (n, p, k) = validate_binomial(n, p, k)?;
    finite(log_binomial_pmf(n, p, k).exp())
}

/// Stable Binomial CDF. Sum the shorter tail from its boundary PMF, avoiding
/// upward recurrence from a probability that has already underflowed to zero.
fn binomial_cumulative_probability(n: f64, p: f64, k: f64) -> Result<f64, String> {
    let (n, p, k) = validate_binomial(n, p, k)?;
    if p == 0.0 { return Ok(1.0); }
    if p == 1.0 { return Ok(if k == n { 1.0 } else { 0.0 }); }
    if k == n { return Ok(1.0); }

    let mode = (((n as f64) + 1.0) * p).floor() as u32;
    let mut sum = 0.0;
    let mut compensation = 0.0;
    if k < mode {
        let mut v = k;
        let mut term = log_binomial_pmf(n, p, v).exp();
        while v > 0 && term > 0.0 {
            let adjusted = term - compensation;
            let next = sum + adjusted;
            compensation = (next - sum) - adjusted;
            sum = next;
            term *= (v as f64) * (1.0 - p) / ((n - v + 1) as f64 * p);
            v -= 1;
        }
        sum += term - compensation;
        finite(sum.clamp(0.0, 1.0))
    } else {
        let mut v = k + 1;
        let mut term = log_binomial_pmf(n, p, v).exp();
        let mut quiet_steps = 0;
        while v < n && term > 0.0 {
            let adjusted = term - compensation;
            let next = sum + adjusted;
            compensation = (next - sum) - adjusted;
            sum = next;
            let next_term = term * ((n - v) as f64) * p / ((v + 1) as f64 * (1.0 - p));
            if next_term <= sum.abs() * 1.0e-16 { quiet_steps += 1; } else { quiet_steps = 0; }
            term = next_term;
            v += 1;
            if quiet_steps >= 4 { break; }
        }
        sum += term - compensation;
        finite((1.0 - sum).clamp(0.0, 1.0))
    }
}

fn log_factorial(n: u32) -> f64 {
    let mut total = 0.0;
    let mut compensation = 0.0;
    for value in 2..=n {
        let adjusted = (value as f64).ln() - compensation;
        let next = total + adjusted;
        compensation = (next - total) - adjusted;
        total = next;
    }
    total
}

fn validate_poisson(lambda: f64, k: f64) -> Result<(f64, u32), String> {
    if !lambda.is_finite() || lambda < 0.0 {
        return Err("Poisson lambda must be non-negative".into());
    }
    Ok((lambda, validate_count(k, 10_000)?))
}

fn log_poisson_pmf(lambda: f64, k: u32) -> f64 {
    if lambda == 0.0 { return if k == 0 { 0.0 } else { f64::NEG_INFINITY }; }
    -lambda + (k as f64) * lambda.ln() - log_factorial(k)
}

fn poisson_probability(lambda: f64, k: f64) -> Result<f64, String> {
    let (lambda, k) = validate_poisson(lambda, k)?;
    finite(log_poisson_pmf(lambda, k).exp())
}

/// Stable Poisson CDF using recurrence from the decreasing side of the
/// distribution. The upper tail is truncated only after it becomes negligible.
fn poisson_cumulative_probability(lambda: f64, k: f64) -> Result<f64, String> {
    let (lambda, k) = validate_poisson(lambda, k)?;
    if lambda == 0.0 { return Ok(1.0); }

    let mut sum = 0.0;
    let mut compensation = 0.0;
    if (k as f64) < lambda {
        let mut v = k;
        let mut term = log_poisson_pmf(lambda, v).exp();
        while term > 0.0 {
            let adjusted = term - compensation;
            let next = sum + adjusted;
            compensation = (next - sum) - adjusted;
            sum = next;
            if v == 0 { break; }
            term *= (v as f64) / lambda;
            v -= 1;
        }
        finite(sum.clamp(0.0, 1.0))
    } else {
        let mut v = k.saturating_add(1);
        let mut term = log_poisson_pmf(lambda, v).exp();
        let mut quiet_steps = 0;
        // In this branch lambda <= k <= 10,000. The hard cap protects the UI.
        while v < 100_000 && term > 0.0 {
            let adjusted = term - compensation;
            let next = sum + adjusted;
            compensation = (next - sum) - adjusted;
            sum = next;
            let next_term = term * lambda / ((v + 1) as f64);
            if next_term <= sum.abs() * 1.0e-16 { quiet_steps += 1; } else { quiet_steps = 0; }
            term = next_term;
            v += 1;
            if quiet_steps >= 4 { break; }
        }
        sum += term - compensation;
        finite((1.0 - sum).clamp(0.0, 1.0))
    }
}

fn erf_approx(x:f64)->f64{let sign=if x<0.0{-1.0}else{1.0};let x=x.abs();let t=1.0/(1.0+0.3275911*x);let y=1.0-(((((1.061405429*t-1.453152027)*t)+1.421413741)*t-0.284496736)*t+0.254829592)*t*(-x*x).exp();sign*y}

fn finite(value: f64) -> Result<f64, String> { if value.is_finite() { Ok(value) } else { Err("Math error: result is undefined or out of range".into()) } }
fn factorial(value: f64) -> Result<f64, String> {
    if value < 0.0 || value.fract() != 0.0 || value > 170.0 { return Err("Factorial requires an integer from 0 to 170".into()); }
    Ok((1..=value as u32).fold(1.0, |acc, n| acc * n as f64))
}
fn permutation(n: f64, r: f64) -> Result<f64, String> {
    if n < 0.0 || r < 0.0 || n.fract()!=0.0 || r.fract()!=0.0 || r>n || n>170.0 { return Err("nPr/nCr requires integers with 0 ≤ r ≤ n ≤ 170".into()); }
    Ok(((n as u32 - r as u32 + 1)..=n as u32).fold(1.0, |acc, v| acc * v as f64))
}
fn gcd_integer(a: f64, b: f64) -> Result<i64, String> {
    if a.fract()!=0.0 || b.fract()!=0.0 || a.abs()>9.0e15 || b.abs()>9.0e15 { return Err("gcd/lcm require safe integers".into()); }
    let (mut x, mut y)=(a.abs() as i64,b.abs() as i64); while y!=0 { let r=x%y; x=y; y=r; } Ok(x)
}

pub fn evaluate(expression: &str, context: EvalContext) -> Result<f64, String> {
    if expression.trim().is_empty() { return Err("Enter an expression".into()); }
    Parser { tokens: tokenize(expression)?, pos: 0, context }.parse()
}

#[derive(Clone, Debug)]
pub struct Statistics { pub count: usize, pub sum: f64, pub mean: f64, pub median: f64, pub min: f64, pub max: f64, pub population_sd: f64, pub sample_sd: Option<f64> }
pub fn statistics(values: &[f64]) -> Result<Statistics, String> {
    if values.is_empty() || values.iter().any(|v| !v.is_finite()) { return Err("Enter at least one valid finite data value".into()); }
    let count=values.len(); let sum: f64=values.iter().sum(); let mean=sum/count as f64;
    let mut sorted=values.to_vec(); sorted.sort_by(f64::total_cmp);
    let variance=values.iter().map(|v|(v-mean).powi(2)).sum::<f64>()/count as f64;
    let sample=if count>1 {Some((values.iter().map(|v|(v-mean).powi(2)).sum::<f64>()/(count-1) as f64).sqrt())} else {None};
    let median=if count%2==0 {sorted[count/2-1]/2.0+sorted[count/2]/2.0} else {sorted[count/2]};
    let population_sd=variance.sqrt();
    if !sum.is_finite() || !mean.is_finite() || !median.is_finite() || !population_sd.is_finite() || sample.is_some_and(|v|!v.is_finite()) {
        return Err("Statistics result is outside the supported numeric range".into());
    }
    Ok(Statistics { count, sum, mean, median, min:sorted[0], max:sorted[count-1], population_sd, sample_sd:sample })
}

pub fn solve_quadratic(a:f64,b:f64,c:f64)->Result<Vec<f64>,String>{
    if !a.is_finite()||!b.is_finite()||!c.is_finite(){return Err("Coefficients must be finite numbers".into());}
    if a==0.0 { if b==0.0 {return Err("This is not a uniquely solvable equation".into());} let root=-c/b; return if root.is_finite() { Ok(vec![root]) } else { Err("Equation root is outside the supported range".into()) }; }
    let d=b*b-4.0*a*c;
    if !d.is_finite() { return Err("Discriminant is outside the supported range".into()); }
    if d<0.0{return Err("No real roots (negative discriminant)".into());}
    if d==0.0 { let root=-b/(2.0*a); if root.is_finite() { Ok(vec![root]) } else { Err("Equation root is outside the supported range".into()) } }
    else if b==0.0 { let root=(-c/a).sqrt(); if root.is_finite() { Ok(vec![-root,root]) } else { Err("Equation roots are outside the supported range".into()) } }
    else { let root=d.sqrt(); let q=-0.5*(b+b.signum()*root); let mut roots=vec![q/a,c/q]; if roots.iter().any(|v|!v.is_finite()){return Err("Equation roots are outside the supported range".into());} roots.sort_by(f64::total_cmp); Ok(roots) }
}

pub fn numerical_derivative(expression:&str,x:f64,angle:AngleUnit,ans:f64)->Result<f64,String>{
    if !x.is_finite(){return Err("Derivative point must be finite".into());}
    let h=f64::EPSILON.cbrt()*(1.0+x.abs());
    let left=evaluate(expression,EvalContext{angle,ans,x:x-h})?;
    let right=evaluate(expression,EvalContext{angle,ans,x:x+h})?;
    finite((right-left)/(2.0*h))
}

pub fn numerical_integral(expression:&str,start:f64,end:f64,steps:usize,angle:AngleUnit,ans:f64)->Result<f64,String>{
    if !start.is_finite()||!end.is_finite(){return Err("Integral bounds must be finite".into());}
    if steps<2||steps>10_000||steps%2!=0{return Err("Simpson integration requires an even step count from 2 to 10000".into());}
    if start==end{return Ok(0.0);}
    let h=(end-start)/steps as f64;
    let mut sum=evaluate(expression,EvalContext{angle,ans,x:start})?+evaluate(expression,EvalContext{angle,ans,x:end})?;
    for i in 1..steps {let x=start+i as f64*h;let y=evaluate(expression,EvalContext{angle,ans,x})?;sum+=if i%2==0{2.0*y}else{4.0*y};}
    finite(sum*h/3.0)
}

pub fn solve_quadratic_inequality(a:f64,b:f64,c:f64,operator:&str)->Result<String,String>{
    if ![a,b,c].iter().all(|v|v.is_finite())||!matches!(operator,">"|">="|"<"|"<="){return Err("Enter finite coefficients and a valid inequality operator".into());}
    let inclusive=operator==">="||operator=="<=";
    let wants_positive=operator==">"||operator==">=";
    let satisfies=|value:f64| if wants_positive {if inclusive{value>=0.0}else{value>0.0}} else if inclusive{value<=0.0}else{value<0.0};
    if a==0.0 {
        if b==0.0{return Ok(if satisfies(c){"All real numbers".into()}else{"No real solution".into()});}
        let root=-c/b;let greater=(b>0.0)==wants_positive;
        return Ok(if greater {format!("x {} {}",if inclusive{">="}else{">"},root)}else{format!("x {} {}",if inclusive{"<="}else{"<"},root)});
    }
    let d=b*b-4.0*a*c;if !d.is_finite(){return Err("Discriminant is outside the supported range".into());}
    if d<0.0{return Ok(if satisfies(a){"All real numbers".into()}else{"No real solution".into()});}
    if d==0.0 {
        let root=-b/(2.0*a);if !root.is_finite(){return Err("Inequality boundary is outside the supported range".into());}let outside=(a>0.0)==wants_positive;
        return Ok(if outside {if inclusive{"All real numbers".into()}else{format!("x ≠ {root}")}} else if inclusive {format!("x = {root}")} else {"No real solution".into()});
    }
    let roots=solve_quadratic(a,b,c)?;let lo=roots[0];let hi=roots[1];let outside=(a>0.0)==wants_positive;
    let left=if inclusive{"≤"}else{"<"};let right=if inclusive{"≥"}else{">"};
    Ok(if outside {format!("x {left} {lo}  or  x {right} {hi}")}else{format!("{lo} {left} x {right} {hi}")})
}

pub fn convert_base(input:&str, from:u32)->Result<String,String>{
    if !(2..=16).contains(&from){return Err("Base must be between 2 and 16".into());}
    let value=i128::from_str_radix(input.trim(),from).map_err(|_|format!("Invalid base-{from} integer"))?;
    Ok(format!("BIN  {}\nOCT  {}\nDEC  {}\nHEX  {}",value_binary(value),format_radix(value,8),value,format_radix(value,16).to_uppercase()))
}
fn value_binary(value:i128)->String{format_radix(value,2)}
fn format_radix(value:i128,base:u32)->String{ let negative=value<0; let mut n=value.unsigned_abs(); if n==0{return "0".into();} let mut out=String::new(); while n>0{let d=(n%base as u128) as u32;out.push(char::from_digit(d,base).unwrap_or('?'));n/=base as u128;} let digits:String=out.chars().rev().collect(); if negative {format!("-{digits}")} else {digits} }

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn scientific_parser_obeys_precedence_and_power_associativity(){let c=EvalContext::default();assert_eq!(evaluate("2+3*4",c).unwrap(),14.0);assert_eq!(evaluate("2^3^2",c).unwrap(),512.0);assert_eq!(evaluate("2(3+4)",c).unwrap(),14.0);}
    #[test] fn scientific_functions_respect_angle_mode(){let mut c=EvalContext::default();assert!((evaluate("sin(30)",c).unwrap()-0.5).abs()<1e-12);c.angle=AngleUnit::Radian;assert!((evaluate("sin(pi/2)",c).unwrap()-1.0).abs()<1e-12);}
    #[test] fn scientific_parser_supports_factorial_percent_and_combinatorics(){let c=EvalContext::default();assert_eq!(evaluate("5!",c).unwrap(),120.0);assert_eq!(evaluate("50%",c).unwrap(),0.5);assert_eq!(evaluate("nCr(5,2)",c).unwrap(),10.0);assert_eq!(evaluate("nPr(5,2)",c).unwrap(),20.0);}
    #[test] fn scientific_parser_rejects_invalid_math(){let c=EvalContext::default();assert!(evaluate("1/0",c).is_err());assert!(evaluate("sqrt(-1)",c).is_err());assert!(evaluate("2+",c).is_err());}
    #[test] fn statistics_calculates_sample_and_population_values(){let s=statistics(&[1.0,2.0,3.0]).unwrap();assert_eq!(s.mean,2.0);assert!((s.population_sd-(2.0_f64/3.0).sqrt()).abs()<1e-12);assert_eq!(s.sample_sd,Some(1.0));}
    #[test] fn quadratic_solver_handles_two_roots_linear_fallback_and_zero_b(){assert_eq!(solve_quadratic(1.0,-3.0,2.0).unwrap(),vec![1.0,2.0]);assert_eq!(solve_quadratic(0.0,2.0,-4.0).unwrap(),vec![2.0]);assert_eq!(solve_quadratic(1.0,0.0,-4.0).unwrap(),vec![-2.0,2.0]);}
    #[test] fn base_conversion_outputs_all_common_radices(){assert_eq!(convert_base("255",10).unwrap(),"BIN  11111111\nOCT  377\nDEC  255\nHEX  FF");}

    #[test]
    fn binomial_large_parameters_use_stable_log_probability() {
        let context = super::EvalContext::default();
        let pmf = super::evaluate("binompdf(10000,0.5,5000)", context).unwrap();
        assert!((pmf - 0.00797864613938215).abs() < 1.0e-12, "{pmf}");
        let cdf = super::evaluate("binomcdf(10000,0.5,5000)", context).unwrap();
        assert!((cdf - 0.503989323069691).abs() < 1.0e-9, "{cdf}");
    }

    #[test]
    fn binomial_probability_handles_degenerate_inputs() {
        let context = super::EvalContext::default();
        assert_eq!(super::evaluate("binompdf(20,0,0)", context).unwrap(), 1.0);
        assert_eq!(super::evaluate("binompdf(20,0,1)", context).unwrap(), 0.0);
        assert_eq!(super::evaluate("binompdf(20,1,20)", context).unwrap(), 1.0);
        assert_eq!(super::evaluate("binomcdf(20,1,19)", context).unwrap(), 0.0);
    }

    #[test]
    fn binomial_and_poisson_cdf_cover_both_tails() {
        let context = super::EvalContext::default();
        let binomial_lower = super::evaluate("binomcdf(10000,0.001,5)", context).unwrap();
        assert!((binomial_lower - 0.06699137339752664).abs() < 1.0e-10, "{binomial_lower}");
        let binomial_upper = super::evaluate("binomcdf(10000,0.999,9990)", context).unwrap();
        assert!((binomial_upper - 0.5421328760209828).abs() < 1.0e-9, "{binomial_upper}");
        let poisson_lower = super::evaluate("poissoncdf(0.25,0)", context).unwrap();
        assert!((poisson_lower - 0.7788007830714049).abs() < 1.0e-12, "{poisson_lower}");
        let poisson_upper = super::evaluate("poissoncdf(10000,10000)", context).unwrap();
        assert!((poisson_upper - 0.5026595812190077).abs() < 2.0e-6, "{poisson_upper}");
    }

    #[test]
    fn poisson_large_lambda_uses_log_factorial_without_intermediate_overflow() {
        let context = super::EvalContext::default();
        let pmf = super::evaluate("poissonpdf(1000,1000)", context).unwrap();
        assert!((pmf - 0.0126146113487217).abs() < 1.0e-12, "{pmf}");
        let cdf = super::evaluate("poissoncdf(1000,1000)", context).unwrap();
        assert!((cdf - 0.508409367168506).abs() < 2.0e-6, "{cdf}");
    }

    #[test]
    fn poisson_probability_handles_zero_rate_and_large_counts() {
        let context = super::EvalContext::default();
        assert_eq!(super::evaluate("poissonpdf(0,0)", context).unwrap(), 1.0);
        assert_eq!(super::evaluate("poissonpdf(0,5)", context).unwrap(), 0.0);
        assert!(super::evaluate("poissonpdf(10000,5000)", context).unwrap().is_finite());
    }


    #[test]
    fn distribution_functions_reject_invalid_domains_and_non_finite_parameters() {
        let context = super::EvalContext::default();
        for expression in [
            "binompdf(-1,0.5,0)", "binompdf(10,1.1,2)", "binompdf(10,0.5,11)",
            "binomcdf(10,0.5,1.5)", "poissonpdf(-0.1,0)", "poissonpdf(2,-1)",
            "poissoncdf(2,1.2)", "normalpdf(0,0,0)", "normalcdf(2,1,0,1)",
        ] {
            assert!(super::evaluate(expression, context).is_err(), "{expression}");
        }
        assert!(super::evaluate("poissonpdf(1/0,2)", context).is_err());
    }

    #[test]
    fn distribution_cdfs_are_bounded_and_monotonic_at_boundaries() {
        let context = super::EvalContext::default();
        let binomial = (0..=30).map(|k| super::evaluate(&format!("binomcdf(30,0.37,{k})"), context).unwrap()).collect::<Vec<_>>();
        let poisson = (0..=30).map(|k| super::evaluate(&format!("poissoncdf(12,{k})"), context).unwrap()).collect::<Vec<_>>();
        for values in [&binomial, &poisson] {
            assert!(values.iter().all(|value| value.is_finite() && (0.0..=1.0).contains(value)));
            assert!(values.windows(2).all(|pair| pair[0] <= pair[1] + 1.0e-14));
        }
        assert_eq!(super::evaluate("binomcdf(30,0.37,30)", context).unwrap(), 1.0);
        assert_eq!(super::evaluate("poissoncdf(0,0)", context).unwrap(), 1.0);
    }

    #[test]
    fn factorial_and_combinatorics_reject_overflow_boundaries() {
        let context = super::EvalContext::default();
        assert_eq!(super::evaluate("0!", context).unwrap(), 1.0);
        assert!(super::evaluate("171!", context).is_err());
        assert!(super::evaluate("(-1)!", context).is_err());
        assert!(super::evaluate("5.5!", context).is_err());
        assert!(super::evaluate("nCr(171,2)", context).is_err());
        assert!(super::evaluate("nPr(5,6)", context).is_err());
    }

    #[test]
    fn base_conversion_rejects_out_of_range_digits_and_bases() {
        assert!(super::convert_base("102", 2).is_err());
        assert!(super::convert_base("10", 1).is_err());
        assert!(super::convert_base("10", 17).is_err());
        assert_eq!(super::convert_base("-FF", 16).unwrap(), "BIN  -11111111\nOCT  -377\nDEC  -255\nHEX  -FF");
    }

}
