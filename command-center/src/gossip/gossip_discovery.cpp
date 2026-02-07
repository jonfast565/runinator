#include "gossip_discovery.h"

#include "models/scheduled_task.h"

#include <QHostAddress>
#include <QJsonDocument>
#include <QJsonObject>

GossipDiscovery::GossipDiscovery(QObject *parent) : QObject(parent) {}

void GossipDiscovery::start() {
  QString bindAddress = qEnvironmentVariable("RUNINATOR_GOSSIP_BIND");
  if (bindAddress.trimmed().isEmpty()) {
    bindAddress = "127.0.0.1";
  }

  QString portVar = qEnvironmentVariable("RUNINATOR_GOSSIP_PORT");
  bool portOk = false;
  quint16 port = portVar.toUShort(&portOk);
  if (portVar.trimmed().isEmpty()) {
    port = 5504;
  } else if (!portOk) {
    port = 5000;
  }

  gossipSocket = new QUdpSocket(this);
  QHostAddress host(bindAddress);
  if (host.isNull()) {
    host = QHostAddress::LocalHost;
  }

  if (!gossipSocket->bind(host, port, QUdpSocket::ShareAddress | QUdpSocket::ReuseAddressHint)) {
    emit errorOccurred(QString("Failed to bind gossip socket: %1").arg(gossipSocket->errorString()));
    return;
  }

  connect(gossipSocket, &QUdpSocket::readyRead, this, &GossipDiscovery::handleGossip);
}

QString GossipDiscovery::currentServiceUrl() const { return serviceBaseUrl; }

void GossipDiscovery::handleGossip() {
  while (gossipSocket->hasPendingDatagrams()) {
    QHostAddress sender;
    QByteArray datagram;
    datagram.resize(static_cast<int>(gossipSocket->pendingDatagramSize()));
    gossipSocket->readDatagram(datagram.data(), datagram.size(), &sender, nullptr);

    QJsonParseError parseError;
    QJsonDocument doc = QJsonDocument::fromJson(datagram, &parseError);
    if (parseError.error != QJsonParseError::NoError || !doc.isObject()) {
      continue;
    }

    const QJsonObject root = doc.object();
    if (root.value("type").toString() != "web_service") {
      continue;
    }
    const QJsonObject svcObj = root.value("service").toObject();
    WebServiceAnnouncement svc;
    svc.serviceId = svcObj.value("service_id").toString();
    svc.address = svcObj.value("address").toString();
    if (svc.address.trimmed().isEmpty()) {
      svc.address = sender.toString();
    }
    svc.port = static_cast<quint16>(svcObj.value("port").toInt());
    svc.basePath = svcObj.value("base_path").toString();
    svc.lastHeartbeat = ScheduledTask::parseOptionalDate(svcObj.value("last_heartbeat"))
                            .value_or(QDateTime::currentDateTimeUtc());

    if (svc.serviceId.isEmpty()) {
      svc.serviceId = QString("%1:%2").arg(svc.address).arg(svc.port);
    }
    services.insert(svc.serviceId, svc);
  }

  updateServiceUrl();
}

void GossipDiscovery::updateServiceUrl() {
  QDateTime bestTime;
  std::optional<WebServiceAnnouncement> best;

  for (const auto &svc : services) {
    if (!bestTime.isValid() || svc.lastHeartbeat > bestTime) {
      bestTime = svc.lastHeartbeat;
      best = svc;
    }
  }

  if (!best.has_value()) {
    return;
  }

  const QString url = buildServiceBaseUrl(best.value());
  if (url == serviceBaseUrl) {
    return;
  }

  serviceBaseUrl = url;
  emit serviceUrlChanged(serviceBaseUrl);
}

QString GossipDiscovery::buildServiceBaseUrl(const WebServiceAnnouncement &svc) const {
  QString base = QString("http://%1:%2").arg(svc.address).arg(svc.port);
  const QString trimmed = svc.basePath.trimmed();
  if (!trimmed.isEmpty()) {
    if (trimmed.startsWith('/')) {
      base += trimmed;
    } else {
      base += "/" + trimmed;
    }
  }
  if (!base.endsWith('/')) {
    base += "/";
  }
  return base;
}
