default: run

run:
    python3 tidings.py

test:
    python3 test_tidings.py
    python3 test_filters.py

lint:
    ruff check .
