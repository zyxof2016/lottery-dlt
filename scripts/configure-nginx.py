#!/usr/bin/env python3
"""Add the DLT proxy to the existing lottery IP vhost without editing other routes."""
import argparse
from pathlib import Path


def configured(text: str) -> str:
    http_anchor = '    location ^~ /.well-known/acme-challenge/ { root /var/www/lottery-k8-acme; }\n'
    tls_anchor = '    client_max_body_size 1m;\n'
    http = '''    # lottery-dlt begin
    location = /dlt { return 301 https://1.14.208.189/dlt/; }
    location /dlt/ { return 301 https://1.14.208.189$request_uri; }
    # lottery-dlt end
'''
    tls = '''    # lottery-dlt begin
    location = /dlt { return 301 /dlt/; }
    location /dlt/ {
        proxy_pass http://127.0.0.1:6792;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-Proto https;
        proxy_read_timeout 600s;
        client_max_body_size 4m;
    }
    # lottery-dlt end
'''
    if text.count('# lottery-dlt begin') == 2 and text.count('# lottery-dlt end') == 2:
        return text
    if 'location /dlt/' in text or 'location = /dlt' in text:
        raise ValueError('Existing DLT routes require inspection; refusing to overwrite them')
    if text.count(http_anchor) != 1 or text.count(tls_anchor) != 1:
        raise ValueError('Existing lottery Nginx config does not match the inspected anchors')
    return text.replace(http_anchor, http_anchor + http).replace(tls_anchor, tls_anchor + tls)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('source', type=Path)
    parser.add_argument('destination', type=Path)
    args = parser.parse_args()
    # Destination is a staged file; production replacement follows nginx -t.
    args.destination.write_text(configured(args.source.read_text()))
