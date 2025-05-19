# Docker Setup for Loom Base

This document provides instructions for running the Loom Base application using Docker.

## Prerequisites

- Docker installed on your system
- Docker Compose installed on your system

## Building and Running with Docker

### Using Docker Compose (Recommended)

1. Make sure you have a valid `config.toml` file in the project root directory.

2. Build and start the container:

```bash
docker-compose up -d
```

3. View logs:

```bash
docker-compose logs -f
```

4. Stop the container:

```bash
docker-compose down
```

### Using Docker Directly

1. Build the Docker image:

```bash
docker build -t loom-base .
```

2. Run the container:

```bash
docker run -d --name loom-base -v $(pwd)/config.toml:/app/config.toml loom-base
```

3. View logs:

```bash
docker logs -f loom-base
```

4. Stop the container:

```bash
docker stop loom-base
docker rm loom-base
```

## Deploying to Northflank

To deploy this Docker container to Northflank:

1. Push your Docker image to a container registry (Docker Hub, GitHub Container Registry, etc.)

```bash
# Tag your image
docker tag loom-base:latest your-registry/loom-base:latest

# Push to registry
docker push your-registry/loom-base:latest
```

2. In Northflank:
   - Create a new service
   - Select "Docker Registry" as the deployment method
   - Enter your image URL (e.g., `your-registry/loom-base:latest`)
   - Configure environment variables as needed
   - Set resource allocation (CPU/RAM)
   - Deploy the service

3. For persistent configuration, you can:
   - Use Northflank's configuration management to store your `config.toml`
   - Mount it as a volume or config map
   - Or build the configuration directly into your Docker image

## Configuration

The application expects a `config.toml` file in the `/app` directory. You can:

1. Include it in the Docker image during build
2. Mount it as a volume when running the container
3. Use environment variables to override configuration (if your application supports it)

## Troubleshooting

If the application fails to start:

1. Check the logs: `docker logs loom-base`
2. Verify your `config.toml` is correctly formatted
3. Ensure any required environment variables are set
4. Check resource allocation (the application may need more memory/CPU)

## Health Checks

The Dockerfile includes a basic health check that verifies the process is running. You can customize this for more sophisticated health monitoring.