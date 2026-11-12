#!/usr/bin/env python3
# Unless explicitly stated otherwise all files in this repository are licensed under the Apache-2.0 License.
# This product includes software developed at Datadog (https://www.datadoghq.com/).
# Copyright 2026-Present Datadog, Inc.

"""Stands in for macos-code-signer in sign-staging.sh tests.

Appends its arguments, and whether it received EXPECTED_TOKEN as
MACOS_SIGNING_SERVICE_TOKEN, to STUB_LOG as one JSON line, then copies its
input (second to last argument) to its output (last argument).
"""

import json
import os
import shutil
import sys

with open(os.environ["STUB_LOG"], "a") as log:
    token_in_env = os.environ.get("MACOS_SIGNING_SERVICE_TOKEN") == os.environ["EXPECTED_TOKEN"]
    log.write(json.dumps({"args": sys.argv[1:], "token_in_env": token_in_env}) + "\n")
shutil.copy(sys.argv[-2], sys.argv[-1])
