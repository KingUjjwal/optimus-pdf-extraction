use optimus_agent::{create_provider, OptimusConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let config = OptimusConfig::from_env();
    println!("API Key set: {}", config.llm.api_key.is_some());
    println!("Base URL: {}", config.llm.base_url);
    println!("Model: {}", config.llm.model);
    println!("Provider enabled: {}", config.llm.provider_enabled);

    if let Some(provider) = create_provider(&config.llm) {
        println!("\nProvider created: {}", provider.model_name());

        let system = "You are a helpful assistant.";
        let user = "Say 'Hello, LLM!' in exactly those words.";

        println!("\nSending request...");
        match provider.complete(system, user).await {
            Ok((response, usage)) => {
                println!("\n=== SUCCESS ===");
                println!("Response: {}", response);
                println!("\nUsage:");
                println!("  Input tokens: {}", usage.input_tokens);
                println!("  Output tokens: {}", usage.output_tokens);
                println!("  Estimated cost: {} cents", usage.estimated_cost_cents);
            }
            Err(e) => {
                eprintln!("\n=== FAILED ===");
                eprintln!("LLM call failed: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        eprintln!("No provider created - API key not set or invalid config");
        std::process::exit(1);
    }

    Ok(())
}
