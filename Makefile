# Local and Development

setup:
	conda create  --name bot --file requirements.txt rust=1.71

activate:
	conda activate .env/bot


test:
	cargo test

format:
	cargo fmt



build-docker:
	## Run Docker locally

	docker compose build

	docker compose up app

