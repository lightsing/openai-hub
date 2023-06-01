# OpenAI Hub
[![Licence](https://img.shields.io/github/license/Ileriayo/markdown-badges?style=for-the-badge)](./LICENSE)
[![Docker](https://img.shields.io/badge/docker-%230db7ed.svg?style=for-the-badge&logo=docker&logoColor=white)](https://hub.docker.com/repository/docker/lightsing/openai-hub)

OpenAI Hub is a comprehensive and robust tool designed to streamline and enhance your interaction with OpenAI's API. It features an innovative way to load balance multiple API keys, allowing users to make requests without needing individual OpenAI API keys. Additionally, it employs a global access control list (ACL) that gives you the power to regulate which APIs and models users can utilize. The Hub also includes JWT Authentication for secure and reliable user authentication, and now, an Access Log feature for tracking API usage and token consumption.

## Key Features
- **Load Balancing:** Utilize multiple API keys efficiently, preventing the overuse of any single key.
- **API Key Protection:** Allow users to make requests without the need for an individual OpenAI API key, enhancing security and ease of use.
- **Global ACL:** Regulate user access to specific APIs and models, ensuring the right people have access to the right resources.
- **JWT Authentication:** Secure and reliable user authentication system using JSON Web Tokens (JWT).
- **Access Log:** Keep track of API usage and token consumption with our newly implemented access log feature. You can choose to store logs in file, SQLite, MySQL, or PostgreSQL backends.

## Getting Started

You can run OpenAI Hub either by cloning the repository and using Cargo, or by using Docker.

### Running with Cargo

```bash
git clone https://github.com/lightsing/openai-hub.git
cd openai-hub

# build and run
cargo run run --bin openai-hubd --all-features --release
```

### Running with Docker

```bash
# pull the Docker image
docker pull lightsing/openai-hub:latest

# run the Docker container
docker run -p 8080:8080 lightsing/openai-hub

# or with your custom configs
docker run -v $(pwd)/config.toml:/opt/openai-hub/config.toml -v $(pwd)/acl.toml:/opt/openai-hub/acl.toml -p <yourport> lightsing/openai-hub
```

Please replace `username` with the appropriate GitHub username.

## Upcoming Features (To-Do List)
- [ ] **Per User/RBAC ACL:** We're developing a more granular access control system to allow permissions to be set on a per-user basis, and Role-Based Access Control (RBAC) to allow users to have roles that define their access levels.

We're always working to improve OpenAI Hub and add new features. Stay tuned for these exciting updates!

## Contributing
We encourage you to contribute to OpenAI Hub! Please check out the [Contributing to OpenAI Hub guide](CONTRIBUTING.md) for guidelines about how to proceed.

## License
OpenAI Hub is released under the [MIT License](LICENSE).

## Contact
If you have any questions, issues, or suggestions for improvement, please feel free to open an issue in this repository or contact us directly.

We're excited to see how you'll use OpenAI Hub!