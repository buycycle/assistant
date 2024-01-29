# buycycle Chatbot
This is a chatbot application for the buycycle platform, built with [Axum](https://github.com/tokio-rs/axum), a modular web framework built with the [Tokio](https://tokio.rs/) async runtime for Rust.

The chatbot is designed to be aware of historical messages, pre-trained on bike knowledge, and integrated with the buycycle stock and user platform interactions.
## Aim
The aim of the buycycle chatbot is to provide customer support by helping users find a fitting bike and share knowledge about how to use the platform effectively. It leverages OpenAI's GPT-4 to generate contextually aware responses, ensuring a helpful and informative interaction with users.


## Features
- Scrape context files from online ressources.
- RESTful API for handling chat sessions.
- Integration with OpenAI's GPT-4 for generating chatbot responses.
- SQLite database for storing conversation history.
- Environment-based configuration using `.env` files.
## Requirements
- Rust 1.56 or higher
- SQLite
## Setup
1. Install Rust by following the instructions on the [official website](https://www.rust-lang.org/tools/install).
2. Clone the repository:
   ```sh
   git clone https://github.com/buycycle/bot
   cd bot
   ```
3. Create a `.env` file in the root of the project with the following content:
   ```env
   DATABASE_URL=sqlite:path/to/your/database.db
   OPENAI_API_KEY=your_openai_api_key
   ```
   Replace `path/to/your/database.db` with the actual path to your SQLite database file and `your_openai_api_key` with your OpenAI API key.
4. Run database migrations (if you have any):
   ```sh
   cargo run --bin migrate
   ```
5. Build and run the application:
   ```sh
   cargo run
   ```
## Usage
To interact with the chatbot, send a `POST` request to the `/assistant` endpoint with a JSON payload containing the `chat_id` and `message`.
Example `curl` request:
```sh
curl -X POST http://localhost:3000/assistant \
-H "Content-Type: application/json" \
-d '{"user_id": "user_123", "message": "Hello, I am looking for a used bike."}'
```
## API Endpoints
- `POST /chat`: Send a message to the chatbot and receive a response.
## Contributing
Contributions are welcome! Please feel free to submit a pull request.
## License
This project is licensed under the [MIT License](LICENSE).
## Acknowledgments
- Thanks to the [Axum](https://github.com/tokio-rs/axum) team for creating a great web framework.
- This project uses the [OpenAI API](https://beta.openai.com/) for generating chatbot responses.


