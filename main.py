#!/usr/bin/env python3
"""CLI entry point for LLM API Proxy"""
import argparse
import os

import uvicorn

from app.core.config import load_config


def main():
    """Main entry point"""
    parser = argparse.ArgumentParser(description='LLM API Proxy Server')
    parser.add_argument('--config', type=str, default='config.yaml',
                       help='Path to configuration file (default: config.yaml)')
    args = parser.parse_args()
    
    config = load_config(args.config)
    
    host = os.environ.get('HOST', config.server.host)
    port = int(os.environ.get('PORT', config.server.port))
    
    print(f"Using config file: {args.config}")
    print(f"Listening on {host}:{port}")
    
    uvicorn.run("app.main:app", host=host, port=port)


if __name__ == '__main__':
    main()