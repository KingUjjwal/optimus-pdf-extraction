use optimus_agent::schema::discover_schema_llm;
use optimus_agent::{create_provider, OptimusConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let config = OptimusConfig::from_env();
    let provider = create_provider(&config.llm);

    if let Some(llm) = provider {
        println!("Testing schema inference with LLM: {}", llm.model_name());
        println!();

        let sample_grid = r#"
MARKET TYPE     QUOTE TYPE
DEAL TYPE      DEAL ID       
ORDER NUMBER   SETTLEMENT NO  
BUYER          SELLER        
ISIN           ISSUER NAME
MATURITY DATE  QUANTITY      
DEAL DATE      PRICE        
TRADE VALUE    ACCRUED
"#;

        match discover_schema_llm(sample_grid, Some(llm.as_ref()), None, None, None).await {
            Ok((schema, usage)) => {
                println!("=== SUCCESS ===");
                println!("Schema: {}", schema);
                println!(
                    "\nToken usage: {} in, {} out",
                    usage.input_tokens, usage.output_tokens
                );
            }
            Err(e) => {
                eprintln!("Schema inference failed: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        eprintln!("No LLM provider available");
        std::process::exit(1);
    }

    Ok(())
}
