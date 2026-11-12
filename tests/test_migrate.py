import sqlite3

import pytest

from code_compass.migrate import read_projects, to_toml


@pytest.fixture
def old_db(tmp_path):
    p = tmp_path / "data.db"
    con = sqlite3.connect(p)
    con.executescript(
        """
        CREATE TABLE categories (id INTEGER PRIMARY KEY AUTOINCREMENT,
            name VARCHAR UNIQUE, is_active BOOLEAN DEFAULT 0);
        CREATE TABLE projects (name VARCHAR, path VARCHAR UNIQUE,
            last_opened TIMESTAMP, category_id INTEGER);
        INSERT INTO categories (name) VALUES ('work');
        INSERT INTO projects VALUES
            ('api', 'C:\\code\\api "x"', '2024-05-01 10:30:00.123456', 1),
            ('orphan', '/tmp/orphan', NULL, NULL);
        """
    )
    con.commit()
    con.close()
    return p


def test_read_projects_maps_category_to_tag(old_db):
    api, orphan = read_projects(old_db)
    assert api["tags"] == ["work"]
    assert isinstance(api["last_opened"], int)
    assert orphan["tags"] == [] and orphan["last_opened"] is None


def test_missing_db_raises(tmp_path):
    with pytest.raises(FileNotFoundError):
        read_projects(tmp_path / "nope.db")


def test_toml_output_is_valid_and_escapes(old_db):
    tomllib = pytest.importorskip("tomllib")  # Python 3.11+
    data = tomllib.loads(to_toml(read_projects(old_db)))
    assert data["project"][0]["path"] == 'C:\\code\\api "x"'
    assert "last_opened" not in data["project"][1]


def test_old_db_is_not_modified(old_db):
    before = old_db.read_bytes()
    read_projects(old_db)
    assert old_db.read_bytes() == before
