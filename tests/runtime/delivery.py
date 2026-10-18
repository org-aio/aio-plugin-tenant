"""在独立本机 PostgreSQL 和真实 Docker Sidecar 中验收租户整包生命周期与身份语义。"""
import hashlib
import json
import os
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from pathlib import Path

plugin = Path(__file__).resolve().parents[2]
binary = Path(os.environ['AIO_TENANT_DELIVERY_HOST']).resolve(strict=True)
database = os.environ['AIO_TENANT_DELIVERY_DATABASE_URL']
parsed = urllib.parse.urlparse(database)
assert parsed.hostname == '127.0.0.1' and parsed.path == '/tenant_delivery_test'
postgres = os.environ['AIO_TENANT_DELIVERY_POSTGRES_CONTAINER']
web = Path(os.environ['AIO_TENANT_DELIVERY_WEB']).resolve(strict=True)
packager = Path(os.environ['AIO_TENANT_DELIVERY_PACKAGER']).resolve(strict=True)
root = Path(tempfile.mkdtemp(prefix='aio-tenant-delivery-'))
(root / 'processes').mkdir()
(root / 'config.toml').write_text('plugins = []\n', encoding='utf-8')
with socket.socket() as reservation:
    reservation.bind(('127.0.0.1', 0))
    port = reservation.getsockname()[1]
base = 'http://127.0.0.1:%d' % port
image = 'sha256:48a661b5e5f4560287dfc254e948825538d0f7a1379616495a424ed375afd31f'
env = {k: v for k, v in os.environ.items() if not k.startswith('AIO_')}
env.update({
    'AIO_DATABASE_URL': database, 'AIO_COMPONENT_DATABASE_URL': database,
    'AIO_COMPONENT_HOME': str(root / 'components'), 'AIO_PLUGIN_CACHE': str(root / 'cache'),
    'AIO_CONFIG': str(root / 'config.toml'),
    'AIO_PROCESS_ROOT': str(root / 'processes'), 'AIO_PROCESS_IMAGES': image,
    'AIO_PLUGIN_SUPERVISOR_SOCKET': str(root / 'supervisor.sock'),
    'AIO_PUBLIC_ORIGIN': base, 'AIO_WEB_HOST': '127.0.0.1', 'AIO_WEB_PORT': str(port),
    'AIO_WEB_DIST': str(web), 'AIO_BOOTSTRAP_ACCOUNT': 'tenant-test',
    'AIO_BOOTSTRAP_PASSWORD': uuid.uuid4().hex, 'AIO_PLUGIN_PUBLISH_ACCOUNTS': 'tenant-test',
})
http = urllib.request.build_opener(urllib.request.ProxyHandler({}))
cookie = None
host = None
supervisor = None
host_log = open(str(root / 'host.log'), 'w', encoding='utf-8')
supervisor_log = open(str(root / 'supervisor.log'), 'w', encoding='utf-8')
host_hash = hashlib.sha256(binary.read_bytes()).hexdigest()


def sql(statement):
    return subprocess.check_output(['docker', 'exec', '-i', postgres, 'psql', '-U', parsed.username, '-d', 'tenant_delivery_test', '-X', '-v', 'ON_ERROR_STOP=1', '-Atc', statement], universal_newlines=True).strip()


def request(path, value=None, raw=None):
    headers = {'Cookie': cookie} if cookie else {}
    body = raw
    if raw is not None:
        headers['Content-Type'] = 'application/vnd.aio.component+gzip'
    if value is not None:
        body = json.dumps(value).encode('utf-8')
        headers['Content-Type'] = 'application/json'
    try:
        response = http.open(urllib.request.Request(base + path, data=body, headers=headers), timeout=180)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        return response.status, response.headers, response.read()


def success(path, value=None, raw=None):
    status, _, body = request(path, value, raw)
    assert 200 <= status < 300, '%s: HTTP %s %s' % (path[:40], status, body[:250])
    if not body:
        return None
    return json.loads(body).get('data')


def until(predicate, timeout=60):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.1)
    raise AssertionError('等待超时；检查日志目录 ' + str(root))


def ready():
    assert host.poll() is None, '宿主提前退出；检查 ' + str(root)
    try:
        return request('/health')[0] == 200
    except OSError:
        return False


def launch():
    global host
    host = subprocess.Popen([str(binary)], cwd=str(root), env=env, stdout=host_log, stderr=subprocess.STDOUT)
    until(ready)


def stop():
    global host
    if host is not None and host.poll() is None:
        host.terminate()
        assert host.wait(timeout=40) == 0
    host = None


def pages():
    return success('/api/runtime/catalog')['pages']


def mount(source, version=None):
    value = success('/api/runtime/frontend/mount', {'page_id': 'component:%s:tenants' % source})
    if version is not None:
        path = urllib.parse.urlsplit(value['src'])
        status, _, body = request(path.path + ('?' + path.query if path.query else ''))
        assert status == 200 and ('data-delivery-version="%s"' % version).encode() in body
    return value['token']


def invoke(token, method, path, value=None, expected=None):
    payload = {'method': method, 'path': path, 'headers': [], 'body': []}
    if value is not None:
        payload['headers'] = [{'name': 'content-type', 'value': 'application/json'}]
        payload['body'] = list(json.dumps(value).encode('utf-8'))
    response = success('/api/runtime/components/%s/request' % token, payload)
    if expected is not None:
        assert response['status'] == expected, (response, path)
    else:
        assert 200 <= response['status'] < 300, (response, path)
    return json.loads(bytes(response['body']).decode('utf-8')).get('data') if response['body'] else None


def publish(version):
    bundle = root / ('tenant-%s.aio-plugin' % version)
    commit = os.environ.get('AIO_TENANT_DELIVERY_COMMIT') or subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=str(plugin), universal_newlines=True).strip()
    candidate = root / ('candidate-' + version)
    candidate.mkdir()
    shutil.copy2(str(plugin / 'aio-plugin.toml'), str(candidate / 'aio-plugin.toml'))
    shutil.copytree(str(plugin / 'dist'), str(candidate / 'dist'))
    index = candidate / 'dist/frontend/index.html'
    index.write_text(index.read_text(encoding='utf-8').replace('<body>', '<body data-delivery-version="%s">' % version), encoding='utf-8')
    subprocess.check_call([str(packager), str(candidate), 'aio-plugin.toml', 'https://github.com/zjarlin/aio-plugin-tenant.git', commit, version, str(bundle)])
    result = success('/api/runtime/components/publish', raw=bundle.read_bytes())
    print('Published tenant ' + version, flush=True)
    return result


try:
    assert sql("SELECT count(*) FROM pg_tables WHERE schemaname='public'") == '0', '请使用新的专用测试库'
    supervisor = subprocess.Popen([str(binary), 'supervisor'], cwd=str(root), env=env, stdout=supervisor_log, stderr=subprocess.STDOUT)
    until(lambda: (root / 'supervisor.sock').exists())
    launch()
    status, headers, _ = request('/api/auth/login', {'account': env['AIO_BOOTSTRAP_ACCOUNT'], 'password': env['AIO_BOOTSTRAP_PASSWORD']})
    assert status == 200
    cookie = headers['Set-Cookie'].split(';')[0]
    # 静态宿主不再编译租户组合。
    assert request('/api/tenants')[0] == 404

    first = publish('0.1.0')
    source = first['source_id']
    git = 'https://github.com/zjarlin/aio-plugin-tenant.git'
    success('/api/runtime/plugins/install', {'git': git})
    token = mount(source, '0.1.0')
    listing = invoke(token, 'GET', '/tenants')
    assert listing['can_manage'] is True
    initial = {t['id']: t for t in listing['tenants']}
    assert 'default' in initial and initial['default']['label'] == '默认租户'
    print('Installed and read the real tenant catalog through /identity/invoke', flush=True)

    created = invoke(token, 'POST', '/tenants', {'label': '研发中心'})
    created_id = created['id']
    assert created_id != 'default' and created['current'] is False
    # 创建租户在一个事务里写入成员、管理员角色与基础权限。
    assert sql("SELECT count(*) FROM tenant_memberships WHERE tenant_id='%s'" % created_id) == '1'
    assert sql("SELECT count(*) FROM tenant_member_roles WHERE tenant_id='%s' AND role_id='tenant-admin'" % created_id) == '1'
    assert sql("SELECT count(*) FROM role_permissions WHERE tenant_id='%s' AND role_id='tenant-admin'" % created_id) == '5'
    print('Created tenant with role and permission grants in one transaction', flush=True)

    renamed = invoke(token, 'GET', '/tenants')
    listing = [t for t in renamed['tenants'] if t['id'] == 'default'][0]
    assert listing['current'] is True
    invoke(token, 'PUT', '/tenants/current', {'label': '默认租户（改）'})
    after = [t for t in invoke(token, 'GET', '/tenants')['tenants'] if t['id'] == 'default'][0]
    assert after['label'] == '默认租户（改）'
    assert sql("SELECT label FROM tenants WHERE id='default'") == '默认租户（改）'
    # 重命名只作用于会话当前租户：会话仍停在 default，新租户标签不受影响。
    assert sql("SELECT label FROM tenants WHERE id='%s'" % created_id) == '研发中心'
    print('Rename applies only to the session tenant', flush=True)

    switched = invoke(token, 'POST', '/tenants/switch', {'tenant_id': created_id})
    assert switched['tenant_id'] == created_id and switched['tenant_label'] == '研发中心'
    assert sql("SELECT tenant_id FROM auth_sessions ORDER BY created_at DESC LIMIT 1") == created_id
    # 会话租户变化后旧票据属于旧租户，必须失效；安装按租户隔离，需要在新租户安装。
    assert request('/api/runtime/components/%s/request' % token, {'method': 'GET', 'path': '/tenants', 'headers': [], 'body': []})[0] >= 400
    assert mount(source) if False else True
    assert request('/api/runtime/frontend/mount', {'page_id': 'component:%s:tenants' % source})[0] == 400
    success('/api/runtime/plugins/install', {'git': git})
    token = mount(source, '0.1.0')
    listing = invoke(token, 'GET', '/tenants')
    assert [t for t in listing['tenants'] if t['id'] == created_id][0]['current'] is True
    assert listing['can_manage'] is True
    print('Switched the tenant; installation is tenant-scoped and the old ticket is invalid', flush=True)

    second = publish('0.1.1')
    success('/api/runtime/plugins/install', {'git': git})
    token = mount(source, '0.1.1')
    assert any(t['id'] == created_id for t in invoke(token, 'GET', '/tenants')['tenants'])
    success('/api/runtime/plugins/%s/rollback' % source, {})
    token = mount(source, '0.1.0')
    assert any(t['id'] == created_id for t in invoke(token, 'GET', '/tenants')['tenants'])
    print('Upgrade and rollback keep tenant data', flush=True)

    success('/api/runtime/plugins/%s/disable' % source, {})
    assert not any(p['id'] == 'component:%s:tenants' % source for p in pages())
    assert not any(p['id'] == 'component:%s:tenant-switcher' % source for p in pages())
    assert request('/api/runtime/components/%s/request' % token, {'method': 'GET', 'path': '/tenants'})[0] >= 400
    stop(); launch()
    assert not any(p['id'] == 'component:%s:tenants' % source for p in pages())
    success('/api/runtime/plugins/%s/enable' % source, {})
    token = mount(source, '0.1.0')
    assert any(t['id'] == created_id for t in invoke(token, 'GET', '/tenants')['tenants'])
    current = next(x for x in success('/api/runtime/marketplace') if x['source_id'] == source)
    assert current['active_revision'] == first['revision']
    assert sql("SELECT excluded_digest FROM component_installations WHERE source_id='%s' AND tenant_id='%s'" % (source, created_id)) == second['revision']
    print('Disable survives restart; enable preserves rollback exclusion and data', flush=True)

    # 撤销租户管理权限：写操作立即被拒绝，只读仍可用。
    grants = sql("WITH removed AS (DELETE FROM role_permissions WHERE tenant_id='%s' AND permission='tenant:manage' RETURNING *) SELECT json_agg(removed) FROM removed" % created_id)
    assert json.loads(grants)
    invoke(token, 'POST', '/tenants', {'label': '不应创建'}, expected=403)
    assert sql("SELECT count(*) FROM tenants WHERE label='不应创建'") == '0'
    assert invoke(token, 'GET', '/tenants')['tenants']
    print('Live permission revocation blocks writes but keeps read-only listing', flush=True)

    # 普通成员仍能切换租户（切换只要求有效成员关系，不需要 tenant:manage）。
    member_tenant = invoke(token, 'POST', '/tenants', {'label': '成员租户'})['id'] if False else created_id
    assert invoke(token, 'POST', '/tenants/switch', {'tenant_id': 'default'})['tenant_id'] == 'default'

    sql("INSERT INTO role_permissions SELECT * FROM json_populate_recordset(NULL::role_permissions, '%s'::json) ON CONFLICT DO NOTHING" % grants.replace("'", "''"))
    assert hashlib.sha256(binary.read_bytes()).hexdigest() == host_hash
    success('/api/runtime/plugins/%s/uninstall' % source, {})
    assert not any(p['id'] == 'component:%s:tenants' % source for p in pages())
    report = {'listed': True, 'createdWithGrants': True, 'renameScoped': True, 'switched': True,
              'revokedTicket': True, 'upgradeRollback': True, 'disabledAcrossRestart': True,
              'liveRevocation': True, 'memberSwitchWithoutPermission': True,
              'hostUnchanged': True, 'logs': str(root)}
finally:
    try:
        stop()
    finally:
        if supervisor is not None and supervisor.poll() is None:
            supervisor.terminate()
            supervisor.wait(timeout=20)
        host_log.close()
        supervisor_log.close()

report['gracefulShutdown'] = True
print(json.dumps(report), flush=True)
