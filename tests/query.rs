use futures::{lock::Mutex, AsyncRead, AsyncWrite};
use futures_util::TryStreamExt;
use names::{Generator, Name};
use once_cell::sync::Lazy;
use std::env;
use std::sync::Once;
use tiberius::{numeric::Numeric, xml::XmlData, ColumnType, QueryItem, Result};
use uuid::Uuid;

use runtimes_macro::test_on_runtimes;

// This is used in the testing macro :)
#[allow(dead_code)]
static LOGGER_SETUP: Once = Once::new();

static CONN_STR: Lazy<String> = Lazy::new(|| {
    env::var("TIBERIUS_TEST_CONNECTION_STRING").unwrap_or_else(|_| {
        "server=tcp:localhost,1433;IntegratedSecurity=true;TrustServerCertificate=true".to_owned()
    })
});

static NAMES: Lazy<Mutex<Generator>> =
    Lazy::new(|| Mutex::new(Generator::with_naming(Name::Plain)));

async fn random_table() -> String {
    NAMES.lock().await.next().unwrap().replace('-', "")
}

static DOT_CONN_STR: Lazy<String> = Lazy::new(|| CONN_STR.replace("localhost", "."));

static ENCRYPTED_CONN_STR: Lazy<String> = Lazy::new(|| format!("{};encrypt=true", *CONN_STR));

static PLAIN_TEXT_CONN_STR: Lazy<String> =
    Lazy::new(|| format!("{};encrypt=DANGER_PLAINTEXT", *CONN_STR));

#[test_on_runtimes(connection_string = "ENCRYPTED_CONN_STR")]
async fn connect_with_full_encryption<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let row = conn
        .query("SELECT @P1", &[&-4i32])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(-4i32), row.get(0));

    Ok(())
}

#[test_on_runtimes(connection_string = "PLAIN_TEXT_CONN_STR")]
async fn connect_as_plain_text<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let row = conn
        .query("SELECT @P1", &[&-4i32])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(-4i32), row.get(0));

    Ok(())
}

#[test_on_runtimes(connection_string = "DOT_CONN_STR")]
async fn connect_dot_server_as_localhost<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let row = conn
        .query("SELECT @P1", &[&-4i32])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(-4i32), row.get(0));

    Ok(())
}

#[test_on_runtimes]
async fn transactions<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    conn.simple_query("BEGIN TRAN").await?;

    let row = conn
        .query("SELECT @P1", &[&-4i32])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(-4i32), row.get(0));

    conn.simple_query("COMMIT").await?;

    Ok(())
}

#[test_on_runtimes]
async fn transactions_300<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    for _ in 1..300 {
        conn.simple_query("BEGIN TRAN").await?;

        let row = conn
            .query("SELECT @P1", &[&-4i32])
            .await?
            .into_row()
            .await?
            .unwrap();

        assert_eq!(Some(-4i32), row.get(0));

        conn.simple_query("COMMIT").await?;
    }

    Ok(())
}

#[test_on_runtimes]
async fn multistatement_query_with_exec_proc<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;
    let proc = random_table().await;

    conn.simple_query(format!(
        r#"
        create table ##{} (
            id int identity(1,1),
            other varchar(50),
        )
    "#,
        table
    ))
    .await?;

    conn.simple_query(format!(
        r#"
        create or alter procedure {} 
          @Param1 varchar(50)
        as
            insert into ##{} (other)
            values (@Param1)

            return scope_identity()
    "#,
        proc, table,
    ))
    .await?;

    let stream = conn
        .query(
            format!(
                "set nocount off; declare @rc int; exec @rc = {} @P1; select @rc as Id;",
                proc
            ),
            &[&"test insert"],
        )
        .await?;

    let row = stream.into_row().await?.unwrap();

    assert_eq!(Some(1), row.get(0));

    let stream = conn
        .query(
            format!(
                "set nocount on; declare @rc int; exec @rc = {} @P1; select @rc as Id;",
                proc
            ),
            &[&"test insert"],
        )
        .await?;

    let row = stream.into_row().await?.unwrap();

    assert_eq!(Some(2), row.get(0));

    Ok(())
}

#[test_on_runtimes]
async fn read_and_write_tinyint<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    conn.execute(format!("CREATE TABLE ##{} (content TINYINT)", table), &[])
        .await?;

    let res = conn
        .execute(
            format!("INSERT INTO ##{} (content) VALUES (@P1), (@P2)", table),
            &[&u8::MIN, &u8::MAX],
        )
        .await?;

    assert_eq!(2, res.total());

    let rows = conn
        .query(format!("SELECT content FROM ##{}", table), &[])
        .await?
        .into_first_result()
        .await?;

    assert_eq!(Some(u8::MIN), rows[0].get(0));
    assert_eq!(Some(u8::MAX), rows[1].get(0));

    Ok(())
}

#[test_on_runtimes]
async fn read_and_write_kanji_varchars<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    conn.execute(
        format!("CREATE TABLE ##{} (content NVARCHAR(max))", table),
        &[],
    )
    .await?;

    let kanji = "余ったものを後で皆に分けようと思っていただけなのに".to_string();
    let long_kanji = "余".repeat(80001);

    let res = conn
        .execute(
            format!("INSERT INTO ##{} (content) VALUES (@P1), (@P2)", table),
            &[&kanji, &long_kanji],
        )
        .await?;

    assert_eq!(2, res.total());

    let rows = conn
        .query(format!("SELECT content FROM ##{}", table), &[])
        .await?
        .into_first_result()
        .await?;

    assert_eq!(Some(kanji.as_str()), rows[0].get(0));
    assert_eq!(Some(long_kanji.as_str()), rows[1].get(0));

    Ok(())
}

#[test_on_runtimes]
async fn read_and_write_weird_garbage<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    conn.execute(
        format!("CREATE TABLE ##{} (content NVARCHAR(1000))", table),
        &[],
    )
    .await?;

    let s = "¥฿😀😁😂😃😄😅😆😇😈😉😊😋😌😍😎😏😐😑😒😓😔😕😖😗😘😙😚😛😜😝😞😟😠😡😢😣😤😥😦😧😨😩😪😫😬😭😮😯😰😱😲😳😴😵😶😷😸😹😺😻😼😽😾😿🙀🙁🙂🙃🙄🙅🙆🙇🙈🙉🙊🙋🙌🙍🙎🙏ऀँंःऄअआइईउऊऋऌऍऎएऐऑऒओऔकखगघङचछजझञटठडढणतथदधनऩपफबभमयर€₭₮₯₰₱₲₳₴₵₶₷₸₹₺₻₼₽₾₿⃀".to_string();

    let res = conn
        .execute(
            format!("INSERT INTO ##{} (content) VALUES (@P1)", table),
            &[&s],
        )
        .await?;

    assert_eq!(1, res.total());

    let rows = conn
        .query(format!("SELECT content FROM ##{}", table), &[])
        .await?
        .into_first_result()
        .await?;

    assert_eq!(Some(s.as_str()), rows[0].get(0));

    Ok(())
}

#[test_on_runtimes]
async fn read_and_write_finnish_varchars<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    conn.execute(
        format!("CREATE TABLE ##{} (content NVARCHAR(max))", table),
        &[],
    )
    .await?;

    let kalevala = "Vaka vanha Väinämöinen / elelevi aikojansa / noilla Väinölän ahoilla, Kalevalan kankahilla.";

    let res = conn
        .execute(
            format!("INSERT INTO ##{} (content) VALUES (@P1)", table),
            &[&kalevala],
        )
        .await?;

    assert_eq!(1, res.total());

    let row = conn
        .query(format!("SELECT content FROM ##{}", table), &[])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(kalevala), row.get(0));

    Ok(())
}

#[test_on_runtimes]
async fn read_and_write_char<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    conn.simple_query(format!("CREATE TABLE ##{} (a char)", table))
        .await?
        .into_results()
        .await?;

    conn.execute(format!("INSERT INTO ##{} (a) values (@P1)", table), &[&"a"])
        .await?;

    let row = conn
        .query(format!("SELECT a FROM ##{}", table), &[])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some("a"), row.get(0));

    Ok(())
}

#[test_on_runtimes]
async fn read_and_write_nchar<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    conn.simple_query(format!("CREATE TABLE ##{} (a nchar)", table))
        .await?
        .into_results()
        .await?;

    conn.execute(format!("INSERT INTO ##{} (a) values (@P1)", table), &[&"ä"])
        .await?;

    let row = conn
        .query(format!("SELECT a FROM ##{}", table), &[])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some("ä"), row.get(0));

    Ok(())
}

#[test_on_runtimes]
async fn execute_insert_update_delete<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    conn.execute(format!("CREATE TABLE ##{} (id int)", table), &[])
        .await?;

    let insert_count = conn
        .execute(
            format!("INSERT INTO ##{} (id) VALUES (@P1), (@P2), (@P3)", table),
            &[&1i32, &2i32, &3i32],
        )
        .await?
        .total();

    assert_eq!(3, insert_count);

    let update_count = conn
        .execute(
            format!("UPDATE ##{} SET id = @P1 WHERE id = @P2", table),
            &[&2i32, &1i32],
        )
        .await?
        .total();

    assert_eq!(1, update_count);

    let delete_count = conn
        .execute(format!("DELETE ##{} WHERE id <> @P1", table), &[&3i32])
        .await?
        .total();

    assert_eq!(2, delete_count);

    Ok(())
}

#[test_on_runtimes]
async fn execute_with_multiple_separate_results<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    conn.execute(format!("CREATE TABLE ##{} (id int)", table), &[])
        .await?;

    let insert_count = conn
        .execute(
            format!("INSERT INTO ##{table} (id) VALUES (@P1); INSERT INTO ##{table} (id) VALUES (@P2), (@P3);", table = table),
            &[&1i32, &2i32, &3i32],
        )
        .await?;

    let result: Vec<_> = insert_count.into_iter().collect();
    assert_eq!(vec![1, 2], result);

    Ok(())
}

#[test_on_runtimes]
async fn execute_multiple_count_total<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    conn.execute(format!("CREATE TABLE ##{} (id int)", table), &[])
        .await?;

    let insert_count = conn
        .execute(
            format!(
                "INSERT INTO ##{table} (id) VALUES (@P1); INSERT INTO ##{table} (id) VALUES (@P2), (@P3);",
                table = table
            ),
            &[&1i32, &2i32, &3i32],
        )
        .await?;

    let result = insert_count.total();
    assert_eq!(3, result);

    Ok(())
}

#[test_on_runtimes]
async fn correct_row_handling_when_not_enough_data<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let mut stream = conn
        .query(
            "SELECT @P1; SELECT @P2;",
            &[&"a".repeat(4001), &"b".repeat(2095)],
        )
        .await?;

    while let Some(item) = stream.try_next().await? {
        match item {
            QueryItem::Metadata(_) => {
                continue;
            }
            QueryItem::Row(row) if row.result_index() == 0 => {
                assert_eq!(Some("a".repeat(4001).as_str()), row.get(0));
            }
            QueryItem::Row(row) => {
                assert_eq!(Some("b".repeat(2095).as_str()), row.get(0));
            }
        }
    }

    Ok(())
}

#[test_on_runtimes]
async fn multiple_stored_procedure_functions<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let stream = conn.query("EXECUTE sp_executesql N'SELECT 1 UNION ALL SELECT 1 UNION ALL SELECT 1'; EXECUTE sp_executesql N'SELECT 2 UNION ALL SELECT 2'", &[]).await?;

    let rows: Result<Vec<i32>> = stream
        .try_filter_map(|item| async move { Ok(item.into_row()) })
        .map_ok(|row| row.get::<i32, _>(0).unwrap())
        .try_collect()
        .await;

    assert_eq!(rows?, vec![1, 1, 1, 2, 2]);

    Ok(())
}

#[test_on_runtimes]
async fn multiple_queries<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let mut stream = conn
        .query("SELECT 'a' AS first; SELECT 'b' AS second;", &[])
        .await?;

    while let Some(item) = stream.try_next().await? {
        match item {
            QueryItem::Metadata(meta) if meta.result_index() == 0 => {
                assert_eq!("first", meta.columns()[0].name());
            }
            QueryItem::Row(row) if row.result_index() == 0 => {
                assert_eq!(Some("a"), row.get(0))
            }
            QueryItem::Metadata(meta) => {
                assert_eq!("second", meta.columns()[0].name());
            }
            QueryItem::Row(row) => {
                assert_eq!(Some("b"), row.get(0))
            }
        }
    }

    Ok(())
}

#[test_on_runtimes]
async fn bool_type<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let row = conn
        .query("SELECT @P1, @P2 ORDER BY 1", &[&false, &true])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(false), row.get(0));
    assert_eq!(Some(true), row.get(1));

    Ok(())
}

#[test_on_runtimes]
async fn u8_token<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let row = conn
        .query("SELECT @P1", &[&4u8])
        .await?
        .into_row()
        .await?
        .unwrap();
    assert_eq!(Some(4u8), row.get(0));
    Ok(())
}

#[test_on_runtimes]
async fn i16_token<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let row = conn
        .query("SELECT @P1", &[&-4i16])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(-4i16), row.get(0));

    Ok(())
}

#[test_on_runtimes]
async fn i32_token<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let row = conn
        .query("SELECT @P1", &[&-4i32])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(-4i32), row.get(0));

    Ok(())
}

#[test_on_runtimes]
async fn i64_token<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let row = conn
        .query("SELECT @P1", &[&-4i64])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(-4i64), row.get(0));

    Ok(())
}

#[test_on_runtimes]
async fn f32_token<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let row = conn
        .query("SELECT @P1", &[&4.20f32])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(4.20f32), row.get(0));

    Ok(())
}

#[test_on_runtimes]
async fn f64_token<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let row = conn
        .query("SELECT @P1", &[&4.20f64])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(4.20f64), row.get(0));

    Ok(())
}

#[test_on_runtimes]
async fn read_nullable_i16<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    conn.simple_query(format!("CREATE TABLE ##{} (a smallint null)", table))
        .await?
        .into_results()
        .await?;

    conn.execute(format!("INSERT INTO ##{} (a) values (null)", table), &[])
        .await?;

    let row = conn
        .query(format!("SELECT a FROM ##{}", table), &[])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Option::<i16>::None, row.get(0));

    Ok(())
}

#[test_on_runtimes]
async fn read_nullable_i32<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    conn.simple_query(format!("CREATE TABLE ##{} (a int null)", table))
        .await?
        .into_results()
        .await?;

    conn.execute(format!("INSERT INTO ##{} (a) values (null)", table), &[])
        .await?;

    let row = conn
        .query(format!("SELECT a FROM ##{}", table), &[])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Option::<i32>::None, row.get(0));

    Ok(())
}

#[test_on_runtimes]
async fn read_nullable_i64<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    conn.simple_query(format!("CREATE TABLE ##{} (a bigint null)", table))
        .await?
        .into_results()
        .await?;

    conn.execute(format!("INSERT INTO ##{} (a) values (null)", table), &[])
        .await?;

    let row = conn
        .query(format!("SELECT a FROM ##{}", table), &[])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Option::<i64>::None, row.get(0));

    Ok(())
}

#[test_on_runtimes]
async fn short_strings<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let s = "Hallo";

    let row = conn
        .query("SELECT @P1", &[&s])
        .await?
        .into_row()
        .await?
        .unwrap();
    assert_eq!(Some(s), row.get::<&str, _>(0));

    Ok(())
}

#[test_on_runtimes]
async fn long_strings<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let string = "a".repeat(4001);

    let row = conn
        .query("SELECT @P1", &[&string.as_str()])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(string.as_str()), row.get(0));
    Ok(())
}

#[test_on_runtimes]
async fn stored_procedures<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let rows = conn
        .query(
            "EXECUTE sp_executesql N'SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3'",
            &[],
        )
        .await?
        .into_first_result()
        .await?;

    assert_eq!(Some(1), rows[0].get(0));
    assert_eq!(Some(2), rows[1].get(0));
    assert_eq!(Some(3), rows[2].get(0));

    Ok(())
}

#[test_on_runtimes]
async fn drop_stream_before_handling_all_results_should_not_cause_weird_things<S>(
    mut conn: tiberius::Client<S>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    {
        let mut stream = conn
            .query(
                "SELECT @P1; SELECT @P2;",
                &[&"a".repeat(8000), &"b".repeat(8001)],
            )
            .await?;

        while let Some(item) = stream.try_next().await? {
            if let QueryItem::Row(row) = item {
                assert_eq!(Some("a".repeat(8000).as_str()), row.get(0));
                break;
            }
        }
    }

    {
        let row = conn
            .query("SELECT @P1", &[&1i32])
            .await?
            .into_row()
            .await?
            .unwrap();
        assert_eq!(Some(1i32), row.get(0));
    }

    Ok(())
}

#[test_on_runtimes]
async fn nbc_rows<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let mut stream = conn
        .query(
            "SELECT NULL, NULL, NULL, NULL, 1, NULL, NULL, NULL, 2, NULL, NULL, 3, NULL, 4",
            &[],
        )
        .await?;

    let expected_results = vec![
        None,
        None,
        None,
        None,
        Some(1),
        None,
        None,
        None,
        Some(2),
        None,
        None,
        Some(3),
        None,
        Some(4),
    ];

    let mut res: Vec<Option<i32>> = Vec::new();

    while let Some(item) = stream.try_next().await? {
        if let QueryItem::Row(row) = item {
            for i in 0..expected_results.len() {
                res.push(row.get(i))
            }
        }
    }

    assert_eq!(expected_results, res);

    Ok(())
}

#[test_on_runtimes]
async fn ntext_type<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    let string = r#"Hääpuhetta voi värittää kertomalla vitsejä, aforismeja,
        sananlaskuja, laulunsäkeitä ja muita lainauksia. Huumori sopii hääpuheeseen,
        mutta vitsit eivät saa loukata. Häissä ensimmäisen juhlapuheen pitää
        perinteisesti morsiamen isä."#;

    conn.execute(format!("CREATE TABLE ##{} (content NTEXT)", table), &[])
        .await?;

    conn.execute(
        format!("INSERT INTO ##{} (content) VALUES (@P1)", table),
        &[&string],
    )
    .await?;

    let row = conn
        .query(format!("SELECT content FROM ##{}", table), &[])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(string), row.get::<&str, _>(0));

    Ok(())
}

#[test_on_runtimes]
async fn ntext_empty<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    conn.execute(format!("CREATE TABLE ##{} (content NTEXT)", table), &[])
        .await?;

    conn.execute(
        format!("INSERT INTO ##{} (content) VALUES (@P1)", table),
        &[&Option::<String>::None],
    )
    .await?;

    let row = conn
        .query(format!("SELECT content FROM ##{}", table), &[])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(None, row.get::<&str, _>(0));

    Ok(())
}

#[test_on_runtimes]
async fn text_type<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;
    let string = "a".repeat(10000);

    conn.execute(format!("CREATE TABLE ##{} (content TEXT)", table), &[])
        .await?;

    conn.execute(
        format!("INSERT INTO ##{} (content) VALUES (@P1)", table),
        &[&string.as_str()],
    )
    .await?;

    let row = conn
        .query(format!("SELECT content FROM ##{}", table), &[])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(string.as_str()), row.get(0));

    Ok(())
}

#[test_on_runtimes]
async fn varchar_empty<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    conn.execute(
        format!("CREATE TABLE ##{} (content NVARCHAR(max))", table),
        &[],
    )
    .await?;

    conn.execute(
        format!("INSERT INTO ##{} (content) VALUES (@P1)", table),
        &[&Option::<String>::None],
    )
    .await?;

    let row = conn
        .query(format!("SELECT content FROM ##{}", table), &[])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(None, row.get::<&str, _>(0));

    Ok(())
}

#[test_on_runtimes]
async fn text_empty<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    conn.execute(format!("CREATE TABLE ##{} (content TEXT)", table), &[])
        .await?;

    conn.execute(
        format!("INSERT INTO ##{} (content) VALUES (@P1)", table),
        &[&Option::<String>::None],
    )
    .await?;

    let row = conn
        .query(format!("SELECT content FROM ##{}", table), &[])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(None, row.get::<&str, _>(0));

    Ok(())
}

#[test_on_runtimes]
async fn varbinary_empty<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    conn.execute(
        format!("CREATE TABLE ##{} (content VARBINARY(max))", table),
        &[],
    )
    .await?;

    let total = conn
        .execute(
            format!("INSERT INTO ##{} (content) VALUES (@P1)", table),
            &[&Option::<Vec<u8>>::None],
        )
        .await?
        .total();

    assert_eq!(1, total);

    let row = conn
        .query(format!("SELECT content FROM ##{}", table), &[])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(None, row.get::<&[u8], _>(0));

    Ok(())
}

#[test_on_runtimes]
async fn binary_type<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    conn.execute(
        format!("CREATE TABLE ##{} (content BINARY(8000))", table),
        &[],
    )
    .await?;

    let mut binary = vec![0; 7999];
    binary.push(5);

    let inserted = conn
        .execute(
            format!("INSERT INTO ##{} (content) VALUES (@P1)", table),
            &[&binary.as_slice()],
        )
        .await?
        .total();

    assert_eq!(1, inserted);

    let row = conn
        .query(format!("SELECT content FROM ##{}", table), &[])
        .await?
        .into_row()
        .await?
        .unwrap();

    let result: &[u8] = row.get(0).unwrap();

    assert_eq!(8000, result.len());
    assert_eq!(binary.as_slice(), result);

    Ok(())
}

#[test_on_runtimes]
async fn varbinary_type<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    conn.execute(
        format!("CREATE TABLE ##{} (content VARBINARY(8000))", table),
        &[],
    )
    .await?;

    let mut binary = vec![0; 79];
    binary.push(5);

    let inserted = conn
        .execute(
            format!("INSERT INTO ##{} (content) VALUES (@P1)", table),
            &[&binary.as_slice()],
        )
        .await?
        .total();

    assert_eq!(1, inserted);

    let row = conn
        .query(format!("SELECT content FROM ##{}", table), &[])
        .await?
        .into_row()
        .await?
        .unwrap();

    let result: &[u8] = row.get(0).unwrap();

    assert_eq!(80, result.len());
    assert_eq!(binary, result);

    Ok(())
}

#[test_on_runtimes]
async fn image_type<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    conn.execute(format!("CREATE TABLE ##{} (content IMAGE)", table), &[])
        .await?;

    let mut binary = vec![0; 79];
    binary.push(5);

    let inserted = conn
        .execute(
            format!("INSERT INTO ##{} (content) VALUES (@P1)", table),
            &[&binary.as_slice()],
        )
        .await?
        .total();

    assert_eq!(1, inserted);

    let row = conn
        .query(format!("SELECT content FROM ##{}", table), &[])
        .await?
        .into_row()
        .await?
        .unwrap();

    let result: &[u8] = row.get(0).unwrap();

    assert_eq!("content", row.columns()[0].name());
    assert_eq!(80, result.len());
    assert_eq!(binary, result);

    Ok(())
}

#[test_on_runtimes]
async fn guid_type_roundtrip<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let id = Uuid::new_v4();
    let row = conn
        .query("SELECT @P1", &[&id])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(id), row.get(0));

    Ok(())
}

#[test_on_runtimes]
async fn guid_type_byte_ordering<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let id = Uuid::parse_str("c97dbc01-fb45-4384-a194-e39a4560cf4a").unwrap();
    let row = conn
        .simple_query("SELECT CAST('c97dbc01-fb45-4384-a194-e39a4560cf4a' AS UNIQUEIDENTIFIER)")
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(id), row.get(0));

    Ok(())
}

#[test_on_runtimes]
async fn varbinary_max<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    conn.execute(
        format!("CREATE TABLE ##{} (content VARBINARY(max))", table),
        &[],
    )
    .await?;

    let mut binary = vec![0; 8000];
    binary.push(5);

    let inserted = conn
        .execute(
            format!("INSERT INTO ##{} (content) VALUES (@P1)", table),
            &[&binary.as_slice()],
        )
        .await?
        .total();

    assert_eq!(1, inserted);

    let row = conn
        .query(format!("SELECT content FROM ##{}", table), &[])
        .await?
        .into_row()
        .await?
        .unwrap();

    let result: &[u8] = row.get(0).unwrap();

    assert_eq!(8001, result.len());
    assert_eq!(binary, result);

    Ok(())
}

#[test_on_runtimes]
async fn numeric_type_u32_presentation<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let num = Numeric::new_with_scale(2, 1);

    let row = conn
        .query("SELECT @P1", &[&num])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(num), row.get(0));

    Ok(())
}

#[test_on_runtimes]
async fn numeric_type_u64_presentation<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let num = Numeric::new_with_scale(std::i32::MAX as i128 + 10, 1);

    let row = conn
        .query("SELECT @P1", &[&num])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(num), row.get(0));

    Ok(())
}

#[test_on_runtimes]
async fn numeric_type_u96_presentation<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let num = Numeric::new_with_scale(std::i64::MAX as i128, 19);

    let row = conn
        .query("SELECT @P1", &[&num])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(num), row.get(0));

    Ok(())
}

#[test_on_runtimes]
async fn numeric_type_u128_presentation<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let num = Numeric::new_with_scale(std::i64::MAX as i128, 37);

    let row = conn
        .query("SELECT @P1", &[&num])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(num), row.get(0));

    Ok(())
}

#[cfg(feature = "rust_decimal")]
#[cfg(test)]
mod rust_decimal {
    use super::*;

    #[test_on_runtimes]
    async fn decimal_type_u32_presentation<S>(mut conn: tiberius::Client<S>) -> Result<()>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        use tiberius::numeric::Decimal;

        let num = Decimal::from_i128_with_scale(2, 1);
        let row = conn
            .query("SELECT @P1", &[&num])
            .await?
            .into_row()
            .await?
            .unwrap();

        assert_eq!(Some(num), row.get(0));

        Ok(())
    }

    #[test_on_runtimes]
    async fn decimal_type_u64_presentation<S>(mut conn: tiberius::Client<S>) -> Result<()>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        use tiberius::numeric::Decimal;

        let num = Decimal::from_i128_with_scale(i32::MAX as i128 + 10, 1);
        let row = conn
            .query("SELECT @P1 AS foo", &[&num])
            .await?
            .into_row()
            .await?
            .unwrap();

        assert_eq!(Some(num), row.get("foo"));

        Ok(())
    }

    #[test_on_runtimes]
    async fn decimal_type_u96_presentation<S>(mut conn: tiberius::Client<S>) -> Result<()>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        use tiberius::numeric::Decimal;

        let num = Decimal::from_i128_with_scale(i64::MAX as i128, 19);
        let row = conn
            .query("SELECT @P1", &[&num])
            .await?
            .into_row()
            .await?
            .unwrap();

        assert_eq!(Some(num), row.get(0));

        Ok(())
    }

    #[test_on_runtimes]
    async fn decimal_type_u128_presentation<S>(mut conn: tiberius::Client<S>) -> Result<()>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        use tiberius::numeric::Decimal;

        let num = Decimal::from_i128_with_scale(i64::MAX as i128, 28);
        let row = conn
            .query("SELECT @P1", &[&num])
            .await?
            .into_row()
            .await?
            .unwrap();

        assert_eq!(Some(num), row.get(0));

        Ok(())
    }
}

#[cfg(feature = "bigdecimal")]
#[cfg(test)]
mod bigdecimal {
    use super::*;

    #[test_on_runtimes]
    async fn bigdecimal_type_u32_presentation<S>(mut conn: tiberius::Client<S>) -> Result<()>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        use std::str::FromStr;
        use tiberius::numeric::BigDecimal;

        let num = BigDecimal::from_str("2").unwrap();
        let row = conn
            .query("SELECT @P1", &[&num])
            .await?
            .into_row()
            .await?
            .unwrap();

        assert_eq!(Some(num), row.get(0));

        Ok(())
    }

    #[test_on_runtimes]
    async fn bigdecimal_type_u64_presentation<S>(mut conn: tiberius::Client<S>) -> Result<()>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        use bigdecimal_::num_bigint::BigInt;
        use num_traits::FromPrimitive;
        use tiberius::numeric::BigDecimal;

        let int = BigInt::from_i128(i32::MAX as i128 + 10).unwrap();
        let num = BigDecimal::new(int, 1);

        let row = conn
            .query("SELECT @P1 AS foo", &[&num])
            .await?
            .into_row()
            .await?
            .unwrap();

        assert_eq!(Some(num), row.get("foo"));

        Ok(())
    }

    #[test_on_runtimes]
    async fn bigdecimal_type_u96_presentation<S>(mut conn: tiberius::Client<S>) -> Result<()>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        use bigdecimal_::num_bigint::BigInt;
        use num_traits::FromPrimitive;
        use tiberius::numeric::BigDecimal;

        let int = BigInt::from_i128(i64::MAX as i128 + 10).unwrap();
        let num = BigDecimal::new(int, 19);

        let row = conn
            .query("SELECT @P1", &[&num])
            .await?
            .into_row()
            .await?
            .unwrap();

        assert_eq!(Some(num), row.get(0));

        Ok(())
    }

    #[test_on_runtimes]
    async fn bigdecimal_type_u128_presentation<S>(mut conn: tiberius::Client<S>) -> Result<()>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        use bigdecimal_::num_bigint::BigInt;
        use num_traits::FromPrimitive;
        use tiberius::numeric::BigDecimal;

        let int = BigInt::from_i128(i64::MAX as i128).unwrap();
        let num = BigDecimal::new(int, 28);

        let row = conn
            .query("SELECT @P1", &[&num])
            .await?
            .into_row()
            .await?
            .unwrap();

        assert_eq!(Some(num), row.get(0));

        Ok(())
    }
}

#[cfg(all(not(feature = "tds73"), feature = "chrono"))]
#[test_on_runtimes]
async fn naive_date_time_tds72<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    use chrono::NaiveDate;

    let dt = NaiveDate::from_ymd(2020, 4, 20).and_hms(16, 20, 0);
    let stream = conn.query("SELECT @P1", &[&dt]).await?;
    let row = stream.into_row().await?.unwrap();

    assert_eq!(Some(dt), row.get(0));

    Ok(())
}

#[cfg(all(feature = "tds73", feature = "chrono"))]
#[test_on_runtimes]
async fn naive_small_date_time_tds73<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    use chrono::NaiveDate;

    let dt = NaiveDate::from_ymd(2020, 4, 20).and_hms(16, 20, 0);
    let table = random_table().await;

    conn.execute(
        format!("CREATE TABLE ##{} (date smalldatetime)", table),
        &[],
    )
    .await?;

    conn.execute(
        format!("INSERT INTO ##{} (date) VALUES (@P1)", table),
        &[&dt],
    )
    .await?
    .total();

    let row = conn
        .query(format!("SELECT date FROM ##{}", table), &[])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(dt), row.get(0));
    Ok(())
}

#[cfg(all(feature = "tds73", feature = "chrono"))]
#[test_on_runtimes]
async fn naive_date_time2_tds73<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    use chrono::NaiveDate;

    let dt = NaiveDate::from_ymd(2020, 4, 20).and_hms(16, 20, 0);
    let table = random_table().await;

    conn.execute(format!("CREATE TABLE ##{} (date datetime2)", table), &[])
        .await?;

    conn.execute(
        format!("INSERT INTO ##{} (date) VALUES (@P1)", table),
        &[&dt],
    )
    .await?
    .total();

    let row = conn
        .query(format!("SELECT date FROM ##{}", table), &[])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(dt), row.get(0));
    Ok(())
}

#[cfg(all(feature = "tds73", feature = "chrono"))]
#[test_on_runtimes]
async fn datetime_as_datetime2_tds73<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let dt = chrono::DateTime::parse_from_rfc3339("2020-02-27T19:10:00Z").unwrap();
    let table = random_table().await;

    conn.execute(format!("CREATE TABLE ##{} (date datetime2)", table), &[])
        .await?;

    conn.execute(
        format!("INSERT INTO ##{} (date) VALUES (@P1)", table),
        &[&dt],
    )
    .await?
    .total();

    let row = conn
        .query(format!("SELECT date FROM ##{}", table), &[])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(dt.naive_utc()), row.get(0));
    Ok(())
}

#[cfg(all(feature = "tds73", feature = "chrono"))]
#[test_on_runtimes]
async fn naive_time<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    use chrono::NaiveTime;

    let time = NaiveTime::from_hms(16, 20, 0);

    let row = conn
        .query("SELECT @P1", &[&time])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(time), row.get(0));

    Ok(())
}

#[cfg(all(feature = "tds73", feature = "chrono"))]
#[test_on_runtimes]
async fn naive_date<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    use chrono::NaiveDate;

    let date = NaiveDate::from_ymd(2020, 4, 20);

    let row = conn
        .query("SELECT @P1", &[&date])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(date), row.get(0));
    Ok(())
}

#[cfg(all(feature = "tds73", feature = "chrono"))]
#[test_on_runtimes]
async fn date_time_utc<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    use chrono::{offset::Utc, DateTime, NaiveDate};

    let naive = NaiveDate::from_ymd(2020, 4, 20).and_hms(16, 20, 0);
    let dt: DateTime<Utc> = DateTime::from_utc(naive, Utc);

    let row = conn
        .query("SELECT @P1", &[&dt])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(dt), row.get(0));

    Ok(())
}

#[cfg(all(feature = "tds73", feature = "chrono"))]
#[test_on_runtimes]
async fn date_time_fixed<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    use chrono::{offset::FixedOffset, DateTime, NaiveDate};

    let naive = NaiveDate::from_ymd(2020, 4, 20).and_hms(16, 20, 0);
    let fixed = FixedOffset::east(3600 * 3);
    let dt: DateTime<FixedOffset> = DateTime::from_utc(naive, fixed);

    let row = conn
        .query("SELECT @P1", &[&dt])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(dt), row.get(0));

    Ok(())
}

#[test_on_runtimes]
async fn xml_read_write<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let xml = XmlData::new("<root><child attr=\"attr-value\"/></root>");
    let row = conn
        .query("SELECT @P1", &[&xml])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(&xml), row.get(0));

    Ok(())
}

#[test_on_runtimes]
async fn xml_read_null_xml<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let row = conn
        .query("SELECT CAST(NULL AS XML)", &[])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(None, row.get::<&XmlData, _>(0));

    Ok(())
}

#[test_on_runtimes]
async fn money_smallmoney<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let table = random_table().await;

    conn.execute(
        format!(
            "CREATE TABLE ##{} (m1 Money NOT NULL, m2 SmallMoney NOT NULL, m3 Money, m4 SmallMoney)",
            table
        ),
        &[],
    )
    .await?;

    let res = conn
        .execute(
            format!(
                "INSERT INTO ##{} (m1, m2, m3, m4) VALUES (@P1, @P2, @P3, @P4)",
                table
            ),
            &[&1.23, &2.33, &4.56, &5.67],
        )
        .await?;

    assert_eq!(1, res.total());

    let row = conn
        .query(format!("SELECT m1, m2, m3, m4 FROM ##{}", table), &[])
        .await?
        .into_row()
        .await?
        .unwrap();

    assert_eq!(Some(1.23), row.get(0));
    assert_eq!(Some(2.33), row.get(1));
    assert_eq!(Some(4.56), row.get(2));
    assert_eq!(Some(5.67), row.get(3));

    Ok(())
}

#[test_on_runtimes]
async fn mars_sp_routines_must_fetch_all_results<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let q = r#"
        DECLARE @SQL NVARCHAR(MAX) = N''
        SELECT @SQL += N'ALTER TABLE '
            + QUOTENAME(OBJECT_SCHEMA_NAME(PARENT_OBJECT_ID))
            + '.'
            + QUOTENAME(OBJECT_NAME(PARENT_OBJECT_ID))
            + ' DROP CONSTRAINT '
            + OBJECT_NAME(OBJECT_ID) + ';'
        FROM SYS.OBJECTS
        WHERE TYPE_DESC LIKE '%CONSTRAINT'
            AND TYPE_DESC <> 'FOREIGN_KEY_CONSTRAINT'
            AND OBJECT_NAME(PARENT_OBJECT_ID) = 'Post'
            AND SCHEMA_NAME(SCHEMA_ID) = 'making_an_existing_id_field_autoincrement_works_with_foreign_keys'
        EXEC sp_execute @SQL
    "#;

    let res = conn.simple_query(q).await;

    assert!(res.is_err());

    Ok(())
}

#[test_on_runtimes]
async fn warnings_should_not_affect_column_fetch<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let proc = random_table().await;

    let q = format!(
        r#"
        CREATE PROCEDURE {}
        AS
        BEGIN
            SET NOCOUNT ON;
            DECLARE @tmp TABLE (col int);
            INSERT INTO @tmp
            SELECT SUM(col) AS col FROM (SELECT NULL AS col UNION SELECT 1) t;
            SELECT SUM(col) AS col FROM @tmp;
        END
    "#,
        proc
    );

    conn.simple_query(&q).await?;

    let mut rs = conn.simple_query(format!("EXEC {}", proc)).await?;

    while let Some(item) = rs.try_next().await? {
        if let QueryItem::Metadata(meta) = item {
            let col = meta.columns().first();

            assert_eq!(Some("col"), col.map(|c| c.name()));
            assert_eq!(Some(ColumnType::Intn), col.map(|c| c.column_type()));
        }
    }

    Ok(())
}

#[test]
#[cfg(feature = "sql-browser-async-std")]
fn cyrillic_collations_should_work() -> Result<()> {
    LOGGER_SETUP.call_once(|| {
        env_logger::init();
    });

    async_std::task::block_on(async {
        let mut admin = {
            let config = tiberius::Config::from_ado_string(&CONN_STR)?;

            let tcp = async_std::net::TcpStream::connect(config.get_addr()).await?;
            tcp.set_nodelay(true)?;

            tiberius::Client::connect(config, tcp).await?
        };

        admin
            .simple_query("CREATE DATABASE ru_test COLLATE Cyrillic_General_CI_AS")
            .await?;

        {
            let mut client = {
                let mut config = tiberius::Config::from_ado_string(&CONN_STR)?;
                config.database("ru_test");

                let tcp = async_std::net::TcpStream::connect(config.get_addr()).await?;
                tcp.set_nodelay(true)?;

                tiberius::Client::connect(config, tcp).await?
            };

            client
                .simple_query(
                    "CREATE TABLE test (id INT IDENTITY PRIMARY KEY, single CHAR(1), multi VARCHAR(255), huge TEXT)",
                )
                .await?;

            client.execute(
                "INSERT INTO test (single, multi, huge) VALUES (@P1, @P2, @P3)",
                &[&"Ж", &"В Советском Союзе попытки борьбы с пьянством предпринимались не единожды. Первая антиалкогольная", &"Первая антиалкогольная"]
            ).await?;

            let row = client
                .query("SELECT single, multi, huge FROM test", &[])
                .await?
                .into_row()
                .await?
                .unwrap();

            assert_eq!(Some("Ж"), row.get(0));
            assert_eq!(Some("В Советском Союзе попытки борьбы с пьянством предпринимались не единожды. Первая антиалкогольная"), row.get(1));
            assert_eq!(Some("Первая антиалкогольная"), row.get(2));
        }

        admin.simple_query("DROP DATABASE ru_test").await?;

        Ok(())
    })
}

#[test]
#[cfg(feature = "sql-browser-async-std")]
fn application_name_should_be_set_correctly() -> Result<()> {
    LOGGER_SETUP.call_once(|| {
        env_logger::init();
    });

    async_std::task::block_on(async {
        let mut config = tiberius::Config::from_ado_string(&CONN_STR)?;
        config.application_name("meow");

        let tcp = async_std::net::TcpStream::connect(config.get_addr()).await?;
        tcp.set_nodelay(true)?;

        let mut client = tiberius::Client::connect(config, tcp).await?;

        let row = client
            .query("SELECT APP_NAME()", &[])
            .await?
            .into_row()
            .await?
            .unwrap();

        assert_eq!(Some("meow"), row.get(0));

        Ok(())
    })
}

#[test_on_runtimes]
async fn columns_fetch_should_work<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let mut stream = conn
        .query("SELECT @P1 AS first; SELECT @P2 AS second", &[&1i32, &2i32])
        .await?;

    // Nothing is fetched, the first result set starts.
    let cols = stream.columns().await?.unwrap();
    assert_eq!("first", cols[0].name());

    // Move over the metadata.
    stream.try_next().await?;

    // We're in the first row, seeing the metadata for that set.
    let cols = stream.columns().await?.unwrap();
    assert_eq!("first", cols[0].name());

    // Move over the only row in the first set.
    stream.try_next().await?;

    // End of the first set, getting the metadata by peaking the next item.
    let cols = stream.columns().await?.unwrap();
    assert_eq!("second", cols[0].name());

    Ok(())
}

#[test_on_runtimes]
async fn into_row_stream_should_work<S>(mut conn: tiberius::Client<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let stream = conn
        .query("SELECT @P1 AS first; SELECT @P2 AS second", &[&1i32, &2i32])
        .await?;

    let mut stream = stream.into_row_stream();

    let row = stream.try_next().await?.unwrap();
    assert_eq!(Some(1), row.get(0));

    let row = stream.try_next().await?.unwrap();
    assert_eq!(Some(2), row.get(0));

    Ok(())
}
