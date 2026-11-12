import subprocess
import sys
import venv
from datetime import datetime
from pathlib import Path

from PySide6 import QtWidgets
from PySide6.QtCore import Qt
from PySide6.QtGui import QCursor, QGuiApplication
from PySide6.QtWidgets import (
    QApplication,
    QDialog,
    QVBoxLayout,
    QTabWidget,
    QTableWidget,
    QTableWidgetItem,
    QComboBox,
    QPushButton,
    QHBoxLayout,
    QLineEdit,
    QInputDialog,
    QWidget,
    QLabel,
    QFileDialog,
    QAbstractItemView,
    QHeaderView,
)
from cookiecutter.main import cookiecutter

from code_compass.category import Category
from code_compass.config import IDE_COMMANDS, COOKIECUTTER, PROJECTS_PATH
from code_compass.db import DB
from code_compass.project import Project


class ProjectManager(QDialog):
    def __init__(self):
        super().__init__()

        # Calculate the width based on the current screen resolution
        cursor_position = QCursor.pos()
        current_screen = QGuiApplication.screenAt(cursor_position)
        self.screen_resolution = current_screen.size()

        self.font_size = (
            self.screen_resolution.width() + self.screen_resolution.height()
        ) // 250

        # Initialize the database
        self.db = DB()
        Category.create_default_if_db_is_empty(self.db)

        # Set the table headers
        self.table_headers = ["Name", "Path", "Days Since Last Access"]

        # Initialize the main window
        self.setWindowTitle("Code Compass (deprecated, use projpick)")
        self.setLayout(QHBoxLayout())

        # Render sections
        self.render_left_section()
        self.render_right_section()

        # add on close event
        self.finished.connect(self.on_close)

        # Render the data
        self.rerender_table()

    # HELPERS

    def add_button(self, label, callback=None, parent_layout=None):
        button = QPushButton(label)
        if callback:
            button.clicked.connect(callback)
        if parent_layout:
            parent_layout.addWidget(button)

    def get_current_tab_name(self):
        current_tab_index = self.tabs.currentIndex()
        current_tab_name = self.tabs.tabText(current_tab_index)
        return current_tab_name

    def get_selected_projects(self):
        selected_table = self.tabs.currentWidget()
        selected_items = selected_table.selectedItems()

        if not selected_items:
            return []

        projects = []
        for item in selected_items:
            row = item.row()
            project_name = selected_table.item(row, 0).text()
            project_path = selected_table.item(row, 1).text()
            project = Project(
                name=project_name,
                path=project_path,
                last_opened=datetime.now(),
                category=Category(id=None, name=self.get_current_tab_name()),
            )
            projects.append(project)
        return projects

    def update_categories(self):
        self.categories = Category.all(self.db)

    # RENDERS

    def render_left_section(self):
        # Left section - Tabs for project categories
        wrapper = QWidget()

        wrapper_width = self.screen_resolution.width() // 3
        wrapper.setFixedWidth(wrapper_width)

        self.left_layout = QVBoxLayout()
        self.tabs = QTabWidget()
        self.tabs.currentChanged.connect(self.rerender_table)
        self.left_layout.addWidget(self.tabs)
        wrapper.setLayout(self.left_layout)
        self.layout().addWidget(wrapper)

        self.rerender_categories()

    def render_right_section(self):
        # Right section - Controls (IDE selector, buttons)
        self.right_layout = QVBoxLayout()
        self.render_ide_selector()
        self.render_projects_buttons()
        self.render_category_buttons()
        self.render_separator()

        self.add_button("Exit", self.close, self.right_layout)

        self.layout().addLayout(self.right_layout)

    def render_ide_selector(self):
        self.ide_selector = QComboBox()
        for ide_command in IDE_COMMANDS:
            self.ide_selector.addItem(ide_command)
        self.right_layout.addWidget(self.ide_selector)

    def render_projects_buttons(self):
        self.right_layout.addWidget(QLabel("Projects"))

        self.add_button("Add", self.show_add_project_dialog, self.right_layout)
        self.add_button(
            "Edit",
            self.show_edit_project_dialog,
            parent_layout=self.right_layout,
        )
        self.add_button(
            "Create",
            self.show_create_project_dialog,
            parent_layout=self.right_layout,
        )
        self.add_button(
            "Delete", self.delete_projects, parent_layout=self.right_layout
        )
        self.add_button(
            "Run", self.run_projects, parent_layout=self.right_layout
        )

    def render_category_buttons(self):
        self.right_layout.addWidget(QLabel("Categories"))
        self.add_button(
            "Create", self.show_create_category_dialog, self.right_layout
        )
        self.add_button("Delete", self.delete_category, self.right_layout)

    def render_separator(self):
        # Add line to separate buttons
        line = QLabel()
        line.setFrameShape(QLabel.HLine)
        line.setFrameShadow(QLabel.Sunken)
        self.right_layout.addWidget(line)

    def render_category(self, name):
        table = self.create_table()
        self.tabs.addTab(table, name)

    def rerender_categories(self):
        self.tabs.clear()
        self.update_categories()

        for category in self.categories:
            self.render_category(category.name)

        self.rerender_table()

        # select active category if there is one
        self.update_categories()
        active_category = Category.get_active(self.db)
        if active_category:
            self.tabs.setCurrentIndex(self.categories.index(active_category))

    def create_table(self):
        table = QTableWidget(0, 3)
        table.setHorizontalHeaderLabels(self.table_headers)
        table.setSelectionBehavior(QAbstractItemView.SelectRows)
        table.horizontalHeader().setSectionResizeMode(
            QtWidgets.QHeaderView.Stretch
        )

        # Make the path column as big as content
        table.horizontalHeader().setSectionResizeMode(
            1, QHeaderView.ResizeToContents
        )
        table.verticalHeader().setVisible(False)

        # Make cell non-editable
        table.setEditTriggers(QTableWidget.NoEditTriggers)

        # Add double click event
        table.doubleClicked.connect(self.run_projects)

        # Add enter key event (for running projects)
        table.keyPressEvent = self.run_projects_on_enter

        return table

    def rerender_table(self):
        selected_table = self.tabs.currentWidget()
        if selected_table is not None:
            selected_table.clear()
            category = Category.get_by_name(
                self.db, self.get_current_tab_name()
            )
            if category:
                projects = Project.all_by_category(
                    self.db,
                    category=Category.get_by_name(
                        self.db, self.get_current_tab_name()
                    ),
                )

                updated_data = [
                    [
                        project.name,
                        project.path,
                        str((datetime.now() - project.last_opened).days),
                    ]
                    for project in projects
                ]

                selected_table.setRowCount(len(updated_data))
                selected_table.setHorizontalHeaderLabels(self.table_headers)
                for row, data_row in enumerate(updated_data):
                    for col, data in enumerate(data_row):
                        selected_table.setItem(
                            row, col, QTableWidgetItem(data)
                        )

    # DIALOGS
    def show_add_project_dialog(self):
        add_dialog = QDialog(self)
        add_dialog.setWindowTitle("Add Project")
        add_dialog.setLayout(QVBoxLayout())

        project_path_label = QLabel("Project Path:")
        project_path_edit = QLineEdit()
        add_dialog.layout().addWidget(project_path_label)
        add_dialog.layout().addWidget(project_path_edit)

        browse_button = QPushButton("Browse")
        add_dialog.layout().addWidget(browse_button)

        project_name_label = QLabel("Project Name:")
        project_name_edit = QLineEdit()
        add_dialog.layout().addWidget(project_name_label)
        add_dialog.layout().addWidget(project_name_edit)

        def browse_directory():
            directory = QFileDialog.getExistingDirectory(
                self, "Select Project Directory", PROJECTS_PATH
            )
            project_path_edit.setText(directory)

        browse_button.clicked.connect(browse_directory)

        category_label = QLabel("Category:")
        category_combo = QComboBox()

        categories = Category.all(self.db)

        for category in categories:
            category_combo.addItem(category.name)

        # Select current category
        category_combo.setCurrentText(self.get_current_tab_name())

        add_dialog.layout().addWidget(category_label)
        add_dialog.layout().addWidget(category_combo)

        add_button = QPushButton("Add")

        def add_project():
            category = Category(id=None, name=category_combo.currentText())
            project_name = (
                project_name_edit.text() or Path(project_path_edit.text()).name
            )
            project = Project(
                name=project_name,
                path=project_path_edit.text(),
                last_opened=datetime.now(),
                category=category,
            )
            project.save(self.db)
            add_dialog.accept()

        add_button.clicked.connect(add_project)

        add_dialog.layout().addWidget(add_button)

        # Add Cancel button
        cancel_button = QPushButton("Cancel")
        cancel_button.clicked.connect(add_dialog.reject)
        add_dialog.layout().addWidget(cancel_button)

        add_dialog.exec()

        self.rerender_table()

    def show_create_project_dialog(self):
        create_dialog = QDialog(self)
        create_dialog.setWindowTitle("Create Project")
        create_dialog.setLayout(QVBoxLayout())

        project_path_label = QLabel("Project Path:")
        project_path_edit = QLineEdit()
        create_dialog.layout().addWidget(project_path_label)
        create_dialog.layout().addWidget(project_path_edit)

        browse_button = QPushButton("Browse")
        create_dialog.layout().addWidget(browse_button)

        project_name_label = QLabel("Project Name:")
        project_name_edit = QLineEdit()
        create_dialog.layout().addWidget(project_name_label)
        create_dialog.layout().addWidget(project_name_edit)

        def browse_directory():
            directory = QFileDialog.getExistingDirectory(
                self, "Select Project Directory", PROJECTS_PATH
            )
            project_path_edit.setText(directory)

        browse_button.clicked.connect(browse_directory)

        category_label = QLabel("Category:")
        category_combo = QComboBox()

        categories = Category.all(self.db)

        for category in categories:
            category_combo.addItem(category.name)

        create_dialog.layout().addWidget(category_label)
        create_dialog.layout().addWidget(category_combo)

        create_button = QPushButton("Create")

        def create_project():
            path = Path(project_path_edit.text())
            project_name = project_name_edit.text() or path.name
            cookiecutter(
                COOKIECUTTER,
                no_input=True,
                output_dir=str(path.absolute().parent),
                extra_context={"project_name": project_name},
                overwrite_if_exists=True,
            )
            venv.create(path.absolute() / "venv", with_pip=True)
            category = Category(id=None, name=category_combo.currentText())
            project = Project(
                name=project_name,
                path=str(path.absolute()),
                last_opened=datetime.now(),
                category=category,
            )
            project.save(self.db)
            create_dialog.accept()

        create_button.clicked.connect(create_project)

        create_dialog.layout().addWidget(create_button)

        # Add Cancel button
        cancel_button = QPushButton("Cancel")
        cancel_button.clicked.connect(create_dialog.reject)
        create_dialog.layout().addWidget(cancel_button)

        create_dialog.exec()

        self.rerender_table()

    def show_edit_project_dialog(self):
        selected_table = self.tabs.currentWidget()
        selected_items = selected_table.selectedItems()

        if not selected_items:
            return

        row = selected_items[0].row()

        project_name = selected_table.item(row, 0).text()
        project_path = selected_table.item(row, 1).text()

        edit_dialog = QDialog(self)
        edit_dialog.setWindowTitle("Edit Project")
        edit_dialog.setLayout(QVBoxLayout())

        project_name_label = QLabel("Project Name:")
        project_name_edit = QLineEdit(project_name)
        edit_dialog.layout().addWidget(project_name_label)
        edit_dialog.layout().addWidget(project_name_edit)

        project_path_label = QLabel("Project Path:")
        project_path_edit = QLineEdit(project_path)
        edit_dialog.layout().addWidget(project_path_label)
        edit_dialog.layout().addWidget(project_path_edit)

        browse_button = QPushButton("Browse")
        edit_dialog.layout().addWidget(browse_button)

        def browse_directory():
            directory = QFileDialog.getExistingDirectory(
                self, "Select Project Directory", PROJECTS_PATH
            )
            project_path_edit.setText(directory)

        browse_button.clicked.connect(browse_directory)

        category_label = QLabel("Category:")
        category_combo = QComboBox()

        for i in range(self.tabs.count()):
            category_combo.addItem(self.tabs.tabText(i))

        current_category = self.get_current_tab_name()
        category_combo.setCurrentText(current_category)

        edit_dialog.layout().addWidget(category_label)
        edit_dialog.layout().addWidget(category_combo)

        save_button = QPushButton("Save")
        edit_dialog.layout().addWidget(save_button)

        def save_changes():
            project = Project(
                name=project_name_edit.text(),
                path=project_path_edit.text(),
                last_opened=datetime.now(),
                category=Category(id=None, name=category_combo.currentText()),
            )
            project.save(self.db)

            edit_dialog.close()

        save_button.clicked.connect(save_changes)

        # Add Cancel button
        cancel_button = QPushButton("Cancel")
        cancel_button.clicked.connect(edit_dialog.reject)
        edit_dialog.layout().addWidget(cancel_button)

        edit_dialog.exec()

        self.rerender_table()

    def show_create_category_dialog(self):
        category_name, ok = QInputDialog.getText(
            self, "Create Category", "Category Name:"
        )
        if ok:
            category = Category(id=None, name=category_name)
            category.save(self.db)
            self.rerender_categories()

            # Select newly created category
            self.tabs.setCurrentIndex(self.categories.index(category))

    # Event handlers

    def run_projects_on_enter(self, event):
        if event.key() == Qt.Key_Return:
            self.run_projects()

    def on_close(self, event):
        # make current category active
        current_category = Category.get_by_name(
            self.db, self.get_current_tab_name()
        )
        current_category.set_active(self.db)

        self.db.close()

    # Button press handlers

    def delete_category(self):
        category_name = self.get_current_tab_name()
        category = Category.get_by_name(self.db, category_name)
        category.delete(self.db)
        self.rerender_categories()

    def run_projects(self):
        projects = self.get_selected_projects()
        arguments = []
        for project in projects:
            arguments.append(project.path)
            project.save(self.db)
        selected_ide = self.ide_selector.currentText()
        subprocess.Popen([selected_ide, *arguments])
        self.close()

    def delete_projects(self):
        projects = self.get_selected_projects()
        for project in projects:
            project.delete(self.db)
        self.rerender_table()


def run():
    app = QApplication(sys.argv)
    project_manager = ProjectManager()
    app.setStyleSheet(
        f"""
        QWidget {{
            font-family: "Roboto";
            font-size: {project_manager.font_size}px;
        }}
        """
    )

    project_manager.show()
    sys.exit(app.exec())


if __name__ == "__main__":
    run()
