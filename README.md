# buycycle Assistant
This is an assistant application for the buycycle platform, built with [Axum](https://github.com/tokio-rs/axum), a modular web framework built with the [Tokio](https://tokio.rs/) async runtime for Rust. Axum provides the robust backend structure, while htmx and JavaScript are used on the frontend to create a dynamic and responsive user interface.
The assistant is designed to be inventory and help article aware, providing users with contextually relevant information based on buycycle's resources. It is integrated with the buycycle stock and user platform interactions, and it is aware of historical messages, pre-trained on bike knowledge.
Check the live version at [assistant.buycycle.com](https://assistant.buycycle.com)

## Aim
The aim of the buycycle assistant is to provide customer support by helping users find a fitting bike and share knowledge about how to use the platform effectively. It leverages OpenAI's GPT-4 to generate contextually aware responses, ensuring a helpful and informative interaction with users.
The assistant enhances the chat interface with dynamic content loading using htmx, allowing for partial page updates and asynchronous form submissions without full page reloads. This creates a seamless and interactive user experience. Client-side interactivity is further enriched with JavaScript, which is used to handle user events, manipulate the DOM, and perform additional logic that complements the htmx functionality.

## Features
- Scrape context files from online resources.
- RESTful API for handling chat sessions.
- Integration with OpenAI's GPT-4 for generating assistant responses.
- Databasei connection for storing conversation history.
- Environment-based configuration using `.env` files.
- Dynamic content loading with htmx for a seamless user experience.
- Client-side interactivity with JavaScript for enhanced chat functionalities.

## Requirements
- Rust 1.56 or higher
- SQLite

## Setup
1. Install Rust by following the instructions on the [official website](https://www.rust-lang.org/tools/install).
2. Clone the repository:
   ```sh
   git clone https://github.com/buycycle/assistant
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

    RUST_LOG=info cargo run #with logging
   ```

## Usage
To interact with the assistant, send a `POST` request to the `/assistant` endpoint with a JSON payload containing the `chat_id` and `message`.
Example `curl` request:
### local
```sh
curl -X POST http://localhost:3000/assistant \
-H "Content-Type: application/x-www-form-urlencoded" \
-d 'user_id=user_123&message=Hello%2C%20I%20am%20looking%20for%20a%20used%20bike.'
```
### dev
```sh
curl -X POST https://assistant.buycycle.com/assistant \
-H "Content-Type: application/x-www-form-urlencoded" \
-d 'user_id=user_123&message=Hello%2C%20I%20am%20looking%20for%20a%20used%20bike.'
```



## API Endpoints
### `GET /health`
Checks the application's health. Returns `200 OK` with the text "OK" if it's running properly.
Expected return:
```
HTTP/1.1 200 OK
content-type: text/plain; charset=utf-8
content-length: 2
date: [Date when the request was processed]
OK
```
### `POST /assistant`
Sends a user message to the assistant. Returns `200 OK` with the assistant's response in JSON format.
Expected return:
```
HTTP/1.1 200 OK
content-type: application/json
content-length: 416
date: Thu, 11 Apr 2024 09:37:37 GMT
{
  "messages": [
    {
      "created_at": 1712828249,
      "role": "assistant",
      "text": "Hi! It's great to hear you're interested in finding a pre-owned bike. Can you tell me what type of riding you're planning to do? That will help me find the right kind of bike for you. We've got road, mountain, gravel, and triathlon bikes. Also, what's your budget? Once I have that info, I can help track down the perfect bike for you on buycycle."
    }
  ]
}
```

### Development Environment
To build and run the assistant application in a development environment with Docker, use the following commands:
1. Build the Docker image for development:
   ```sh
   docker build -t buycycle-bot-dev -f docker/dev.dockerfile .
   ```
2. Run the Docker container with live code reloading:
   ```sh
   docker run -it --rm -v "$(pwd)":/usr/src/rust_bot -p 3000:3000 buycycle-bot-dev
   ```
This will start the assistant application on port 3000 with live code reloading enabled. Any changes you make to the source code will automatically trigger a recompilation and restart of the application.
### Production (Main) Environment
To build and run the assistant application in a production environment with Docker, use the following commands:
1. Build the Docker image for production:
   ```sh
   docker build -t buycycle-bot-main -f docker/main.dockerfile .
   ```
2. Run the Docker container:
   ```sh
   docker run -d --rm -p 3000:3000 buycycle-bot-main
   ```
This will start the assistant application as a detached process on port 8000. The application will run with the optimizations and configurations suitable for a production environment.

# CI/CD
Pipeline with gitlab and helm.

1. get eks-access credentials from aws add to ~/.aws/credentials
2. aws eks update-kubeconfig --name buycycle-cluster --region eu-central-1 profile xx
3. add namespace for convinience, kubectl config set-context --current --namespace=dev
4. check pods on dev kubectl get pod -n dev | grep chat
5. check real time logs pod/<pod> -n dev -f
6. start terminal in pod,  kubectl exec -it -n dev pod/<pod> -- /bin/bash
7. get service, kubectl get svc -n dev
8. port forwarding on local, kubectl port-forward svc/<service> -n live <local_port>:80




## Contributing
Contributions are welcome! Please feel free to submit a pull request.

## License
This project is licensed under the [MIT License](LICENSE).

## Acknowledgments
- Thanks to the [Axum](https://github.com/tokio-rs/axum) team for creating a great web framework.
- This project uses the [OpenAI API](https://beta.openai.com/) for generating assistant responses.


