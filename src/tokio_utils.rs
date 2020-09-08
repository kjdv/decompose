extern crate futures;
extern crate tokio;

pub type Result<T> = std::result::Result<T, tokio::io::Error>;

pub fn run<F: futures::future::Future>(f: F) -> F::Output {
    let mut rt = tokio::runtime::Builder::new()
        .basic_scheduler()
        .enable_all()
        .build()
        .expect("runtime");

    let result = rt.block_on(f);
    rt.shutdown_timeout(std::time::Duration::from_secs(1));
    result
}

pub fn make_err<E>(e: E) -> tokio::io::Error
where
    E: Into<Box<dyn std::error::Error + 'static + Sync + Send>>,
{
    use tokio::io::{Error, ErrorKind};

    Error::new(ErrorKind::Other, e)
}

pub async fn with_timeout<R>(
    f: impl futures::future::Future<Output = Result<R>>,
    timeout: std::time::Duration,
) -> Result<R> {
    tokio::select! {
        x = f => x,
        _ = tokio::time::delay_for(timeout) => {
            Err(make_err("timeout"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run() {
        let r = run(futures::future::ready(42));
        assert_eq!(42, r);

        let r = run(async move { 43 });
        assert_eq!(43, r);
    }

    #[test]
    fn test_make_err() {
        let err = make_err("booh!");
        assert_eq!("booh!", format!("{}", err));
    }

    #[test]
    fn test_with_timeout() {
        let r = run(with_timeout(
            futures::future::ready(Ok(42)),
            std::time::Duration::from_secs(60),
        ));
        assert_eq!(42, r.expect("ok"));

        let r = run(with_timeout(
            futures::future::pending::<Result<i32>>(),
            std::time::Duration::from_nanos(1),
        ));
        assert_eq!("timeout", format!("{}", r.expect_err("err")));
    }
}
