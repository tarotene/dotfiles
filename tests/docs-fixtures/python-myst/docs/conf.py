project = "docs-myst-fixture"
extensions = ["myst_parser", "sphinx.ext.autodoc", "sphinx.ext.apidoc", "sphinx.ext.doctest"]
apidoc_modules = [{"path": "../src/docs_myst_fixture", "destination": "api"}]
doctest_global_setup = "from docs_myst_fixture import *"
source_suffix = {".rst": "restructuredtext", ".md": "markdown"}
