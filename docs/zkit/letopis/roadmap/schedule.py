#!/usr/bin/env python3
"""Generate/check the weekly development plan from roadmap.md; no runtime probes."""
import argparse
import json
import os
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BEGIN, END = '<!-- weeks:begin -->', '<!-- weeks:end -->'
JSON_PATH = ROOT / 'checks/roadmap' / 'roadmap-weekly-schedule.json'


def parse_tasks(document):
    tasks = []
    header = None
    for line in document.splitlines():
        if not line.startswith('|'):
            header = None
            continue
        cells = [cell.strip() for cell in line.strip('|').split('|')]
        if cells[0] == 'ID':
            header = cells
        elif header and re.fullmatch(r'TK-\d+', cells[0]):
            row = dict(zip(header, cells))
            if row['Wait (weeks)'] != '0':
                raise ValueError('Nonzero waits require a revised calendar: ' + row['ID'])
            target = re.fullmatch(r'\[([^\]]+)\]\(([^)]+)\)', row['Task'])
            if target is None:
                raise ValueError('Task must link to its card: ' + row['ID'])
            if not re.fullmatch(r'EP-\d+', row.get('Epic', '')):
                raise ValueError('Task must name exactly one epic: ' + row['ID'])
            if row["Acceptance"] != "[Критерии приёмки](%s)" % target.group(2):
                raise ValueError("Acceptance must link to the same task card: " + row["ID"])
            tasks.append({
                'id': row['ID'], 'name': row['Task'], 'phase': row['Phase'],
                'epic': row['Epic'], 'path': target.group(2), 'title': target.group(1),
                'best': int(row['Best']), 'expected': int(row['Expected']),
                'worst': int(row['Worst']),
                'depends': re.findall(r'TK-\d+', row['Depends']),
            })
    if not tasks or len({task['id'] for task in tasks}) != len(tasks):
        raise ValueError('Missing tasks or duplicate task IDs')
    return tasks


def make_schedule(document):
    tasks = parse_tasks(document)
    by_id = {task['id']: task for task in tasks}
    terminal_match = re.search(r'\*\*Terminal:\*\*\s*(TK-\d+)', document)
    if terminal_match is None:
        raise ValueError('Missing terminal task declaration')
    terminal = terminal_match.group(1)
    capacities = re.findall(r'^\| (?:best|expected|worst) \| (\d+) \|', document, re.M)
    if len(capacities) != 3 or set(capacities) != {'10'}:
        raise ValueError('Weekly plan requires the accepted capacity: 10 hours/week')
    capacity = 10
    visited, visiting, order, ancestors = set(), set(), [], {}

    def visit(task_id):
        if task_id in visiting:
            raise ValueError('Dependency cycle: ' + task_id)
        if task_id in visited:
            return
        if task_id not in by_id:
            raise ValueError('Unknown dependency: ' + task_id)
        visiting.add(task_id)
        task = by_id[task_id]
        if not 0 < task['best'] <= task['expected'] <= task['worst']:
            raise ValueError('Invalid hour range: ' + task_id)
        if not 4 <= task['expected'] <= 16:
            raise ValueError('Expected task hours outside 4..16: ' + task_id)
        dependency_set = set()
        for dependency in task['depends']:
            visit(dependency)
            dependency_set.update(ancestors[dependency])
            dependency_set.add(dependency)
        ancestors[task_id] = dependency_set
        visiting.remove(task_id)
        visited.add(task_id)
        order.append(task_id)

    for task in tasks:
        visit(task['id'])
    if order[-1] != terminal or ancestors[terminal] != set(by_id) - {terminal}:
        raise ValueError('Final task must follow every other task transitively')
    phase_number = lambda task: int(task['phase'].split('-')[1])
    for task in tasks:
        earlier = {other['id'] for other in tasks if phase_number(other) < phase_number(task)}
        if not earlier <= ancestors[task['id']]:
            raise ValueError('Phase gate dependency missing: ' + task['id'])

    weeks, completed_hours = [], {task_id: 0 for task_id in by_id}
    for task_id in order:
        task = by_id[task_id]
        remaining = task['expected']
        while remaining:
            if not weeks or weeks[-1]['hours'] == capacity:
                weeks.append({'week': len(weeks) + 1, 'hours': 0,
                              'allocations': [], 'finishes': []})
            week = weeks[-1]
            hours = min(remaining, capacity - week['hours'])
            completed_hours[task_id] += hours
            week['allocations'].append({'task': task_id, 'hours': hours,
                                         'cumulative': completed_hours[task_id]})
            week['hours'] += hours
            remaining -= hours
            if remaining == 0:
                week['finishes'].append(task_id)
    expected = sum(task['expected'] for task in tasks)
    if any(not 0 < week['hours'] <= capacity for week in weeks):
        raise ValueError('Invalid weekly load')
    if sum(week['hours'] for week in weeks) != expected:
        raise ValueError('Missing or duplicated scheduled hours')
    if any(completed_hours[t['id']] != t['expected'] for t in tasks):
        raise ValueError('A task has missing or duplicated allocations')
    if weeks[-1]['finishes'][-1] != terminal:
        raise ValueError('Schedule does not end in terminal acceptance')
    return {'scope': 'Expected development plan only; no execution or measurements',
            'capacity_hours_per_week': capacity, 'task_count': len(tasks),
            'total_hours': expected, 'weeks': weeks, 'terminal': terminal}, by_id


def markdown(schedule, by_id):
    lines = ['| Неделя | Задачи и часы этой недели | Всего, ч | Планируемое завершение |',
             '|---|---|---|---|']
    for week in schedule['weeks']:
        allocations = []
        for allocation in week['allocations']:
            task = by_id[allocation['task']]
            part = ''
            if allocation['hours'] != task['expected']:
                part = ' (накоплено %s/%s ч)' % (allocation['cumulative'], task['expected'])
            allocations.append('**%s** %s — %s ч%s' %
                               (task['id'], task['name'], allocation['hours'], part))
        outcome = ', '.join(week['finishes']) or 'Продолжение начатой задачи'
        if schedule['terminal'] in week['finishes']:
            outcome = '**%s: v1 готова — при выполненных критериях приёмки**' % schedule['terminal']
        lines.append('| %s | %s | %s | %s |' %
                     (week['week'], '; '.join(allocations), week['hours'], outcome))
    return '\n'.join(lines)


def check_task_cards(schedule, by_id):
    """Keep the task cards consistent with the authoritative scheduling table."""
    paths = {task_id: ROOT / task['path'] for task_id, task in by_id.items()}
    discovered = list(ROOT.glob('roadmap/epics/EP-*/tasks/TK-*.md'))
    if len(discovered) != len(paths) or set(discovered) != set(paths.values()):
        raise ValueError('Exactly one card per task is required in roadmap/epics/')
    for task_id, path in paths.items():
        task = by_id[task_id]
        if (not re.fullmatch(re.escape(task_id) + r'-[a-z0-9-]+\.md', path.name)
                or path.parent.name != 'tasks'
                or not path.parents[1].name.startswith(task['epic'] + '-')
                or not path.is_file()):
            raise ValueError('Task ID/path/epic mismatch: ' + task_id)
    sections = ['Цель', 'Контекст и входные данные', 'Что сделать',
                'Ожидаемый результат', 'Критерии приёмки', 'Как проверить',
                'Границы', 'Риски и примечания', 'Отчёт о выполнении']
    allocations = {task_id: [] for task_id in by_id}
    for week in schedule['weeks']:
        for allocation in week['allocations']:
            allocations[allocation['task']].append((week['week'], allocation['hours']))
    for task_id, path in paths.items():
        text = path.read_text(encoding='utf-8')
        task = by_id[task_id]
        name = task['title']
        if not text.startswith('# %s — %s\n' % (task_id, name)):
            raise ValueError('Task title/ID differs from roadmap: ' + task_id)
        status = re.search(r'^> \*\*Статус:\*\* (\w+)$', text, re.M)
        if status is None or status.group(1) not in {'planned', 'in_progress', 'blocked', 'done'}:
            raise ValueError('Unknown task status: ' + task_id)
        required = [
            '> **Эпик:** [%s](../epic.md)' % task['epic'],
            '> **Фаза:** ' + task['phase'],
            '> **Оценка, ч:** %s / %s / %s (best / expected / worst)' %
            (task['best'], task['expected'], task['worst']),
            '> **План:** ' + '; '.join('неделя %s — %s ч' % pair
                                       for pair in allocations[task_id]),
            '> **Зависимости:** ' + (', '.join('[%s](%s)' %
                (dep, os.path.relpath(paths[dep], path.parent).replace(os.sep, '/'))
                for dep in task['depends']) or '—'),
        ]
        if any(line not in text.splitlines() for line in required):
            raise ValueError('Task scheduling metadata is stale: ' + task_id)
        for number, section in enumerate(sections, 1):
            match = re.search(r'^## %s\. %s\n(.*?)(?=^## |\Z)' %
                              (number, re.escape(section)), text, re.M | re.S)
            if match is None or not match.group(1).strip():
                raise ValueError('Missing task section %s: %s' % (section, task_id))
        boundaries = re.search(r'^## 7\. Границы\n(.*?)(?=^## |\Z)', text, re.M | re.S).group(1)
        for label in ['Входит', 'Не входит', 'Интерфейсы — вход', 'Интерфейсы — выход']:
            if not re.search(r'^- \*\*' + re.escape(label) + r':\*\* \S', boundaries, re.M):
                raise ValueError('Missing boundary ' + label + ': ' + task_id)
        if '**Основание оценки ' not in text or '**Риск и действие:**' not in text:
            raise ValueError('Missing range rationale or risk response: ' + task_id)
        if '- [ ] ' not in text and '- [x] ' not in text:
            raise ValueError('Task has no acceptance checklist: ' + task_id)
    # Container hours must exactly match child tasks; they are never scheduled twice.
    document = (ROOT / 'roadmap.md').read_text(encoding='utf-8')
    epics = {}
    header = None
    for line in document.splitlines():
        if not line.startswith('|'):
            header = None
            continue
        cells = [cell.strip() for cell in line.strip('|').split('|')]
        if cells[0] == 'ID':
            header = cells
        elif header and re.fullmatch(r'EP-\d+', cells[0]):
            if cells[0] in epics:
                raise ValueError('Duplicate epic: ' + cells[0])
            epics[cells[0]] = dict(zip(header, cells))
    if set(epics) != {task['epic'] for task in by_id.values()}:
        raise ValueError('Every epic must be a populated task container')
    epic_paths = []
    for epic_id, row in epics.items():
        match = re.fullmatch(r'\[([^\]]+)\]\(([^)]+)\)', row['Epic'])
        if match is None:
            raise ValueError('Epic must link to epic.md: ' + epic_id)
        path = ROOT / match.group(2)
        epic_paths.append(path)
        children = [task for task in by_id.values() if task['epic'] == epic_id]
        if path.name != 'epic.md' or not path.parent.name.startswith(epic_id + '-'):
            raise ValueError('Epic path mismatch: ' + epic_id)
        content = path.read_text(encoding='utf-8')
        if not content.startswith('# %s — %s\n' % (epic_id, match.group(1))):
            raise ValueError('Epic title mismatch: ' + epic_id)
        if {task['phase'] for task in children} != {row['Phase']}:
            raise ValueError('Epic phase mismatch: ' + epic_id)
        total = [sum(task[k] for task in children) for k in ['best', 'expected', 'worst']]
        if total != [int(row[k]) for k in ['Best', 'Expected', 'Worst']]:
            raise ValueError('Epic hours differ from child sum: ' + epic_id)
        expected_dependencies = sorted({dep for task in children for dep in task['depends']}
                                       - {task['id'] for task in children},
                                       key=lambda value: int(value.split('-')[1]))
        if re.findall(r'TK-\d+', row['Depends']) != expected_dependencies:
            raise ValueError('Epic dependencies differ from children: ' + epic_id)
        required_epic = [
            '> **Фаза:** ' + row['Phase'],
            '> **Оценка, ч:** %s / %s / %s (best / expected / worst)' % tuple(total),
            '> **Зависимости:** ' + (', '.join('[%s](%s)' %
                (dep, os.path.relpath(paths[dep], path.parent).replace(os.sep, '/'))
                for dep in expected_dependencies) or '—'),
        ]
        if any(line not in content.splitlines() for line in required_epic):
            raise ValueError('Epic metadata differs from table: ' + epic_id)
        for section in ['Цель', 'Границы', 'Задачи', 'Приёмка', 'Оценка и риски']:
            if not re.search(r'^## ' + re.escape(section) + r'\n\s*\S', content, re.M):
                raise ValueError('Missing epic section ' + section + ': ' + epic_id)
        task_section = re.search(r'^## Задачи\n(.*?)(?=^## |\Z)', content, re.M | re.S).group(1)
        if re.findall(r'^- \[([T][K]-\d+)', task_section, re.M) != [task['id'] for task in children]:
            raise ValueError('Epic task list differs from table: ' + epic_id)
    if set(ROOT.glob('roadmap/epics/EP-*/epic.md')) != set(epic_paths):
        raise ValueError('Unexpected epic file')
    return len(paths)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--update', action='store_true')
    mode.add_argument('--check', action='store_true')
    args = parser.parse_args()
    path = ROOT / 'roadmap.md'
    document = path.read_text(encoding='utf-8')
    schedule, by_id = make_schedule(document)
    card_count = check_task_cards(schedule, by_id)
    block = markdown(schedule, by_id)
    match = re.search(re.escape(BEGIN) + r'(.*?)' + re.escape(END), document, re.S)
    if match is None:
        raise ValueError('Missing generated weeks block')
    if args.update:
        document = document[:match.start(1)] + '\n' + block + '\n' + document[match.end(1):]
        path.write_text(document, encoding='utf-8')
        JSON_PATH.parent.mkdir(parents=True, exist_ok=True)
        JSON_PATH.write_text(json.dumps(schedule, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    else:
        if match.group(1).strip() != block:
            raise ValueError('Weekly block is stale; run --update')
        if json.loads(JSON_PATH.read_text(encoding='utf-8')) != schedule:
            raise ValueError('Weekly JSON is stale; run --update')
    print(json.dumps({'tasks': schedule['task_count'], 'hours': schedule['total_hours'],
                      'task_cards': card_count,
                      'weeks': len(schedule['weeks']), 'max_week_hours': 10,
                      'last_week_hours': schedule['weeks'][-1]['hours'],
                      'terminal': schedule['terminal'], 'dependency_errors': 0,
                      'phase_gate_errors': 0, 'unscheduled_tasks': 0,
                      'runtime_calls': 0, 'measurements': 0}, ensure_ascii=False))


if __name__ == '__main__':
    main()
